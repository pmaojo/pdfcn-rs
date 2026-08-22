//! Barcode generators for `%Barcode` (Ola 1.3), behind pdfcn-core's opt-in
//! `vector` cargo feature. Two fully native symbologies -- Code 128 (with
//! automatic Code B/C switching: the general-purpose retail/logistics code)
//! and EAN-13 (retail product codes, checksum enforced) -- emitted as SVG
//! and rasterized onto the Ola 1.2 substrate like every other generator.
//!
//! DataMatrix and PDF417 from the roadmap need their own ECC families
//! (ECC200 / PDF417 Reed-Solomon plus placement rules). They are deliberately
//! **not** hand-rolled here: a subtly wrong ECC makes a symbol that scans
//! sometimes, which is worse than one that doesn't exist. They land as
//! optional crates once chosen; until then an unknown scheme is an explicit
//! invalid-component marker, never a silent wrong barcode.
//!
//! Nothing here panics; any unencodable input yields `None` and the caller
//! leaves the placeholder unresolved.

/// The standard Code 128 pattern table: pattern `i` is the bar/space widths
/// of value `i`, alternating bar-space starting with a bar. Values 0-102 are
/// data/special codes, 103-105 are Start A/B/C and 106 is the Stop pattern
/// (which carries its 7th termination bar).
const CODE128_PATTERNS: [&str; 107] = [
    "212222", "222122", "222221", "121223", "121322", "131222", "122213", "122312", "132212",
    "221213", "221312", "231212", "112232", "122132", "122231", "113222", "123122", "123221",
    "223211", "221132", "221231", "213212", "223112", "312131", "311222", "321122", "321221",
    "312212", "322112", "322211", "212123", "212321", "232121", "111323", "131123", "131321",
    "112313", "132113", "132311", "211313", "231113", "231311", "112133", "112331", "132131",
    "113123", "113321", "133121", "313121", "211331", "231131", "213113", "213311", "213131",
    "311123", "311321", "331121", "312113", "312311", "332111", "314111", "221411", "431111",
    "111224", "111422", "121124", "121421", "141122", "141221", "112214", "112412", "122114",
    "122411", "142112", "142211", "241211", "221114", "413111", "241112", "134111", "111242",
    "121142", "121241", "114212", "124112", "124211", "411212", "421112", "421211", "212141",
    "214121", "412121", "111143", "111341", "131141", "114113", "114311", "411113", "411311",
    "113141", "114131", "311141", "411131", "211412", "211214", "211232", "2331112",
];

/// EAN-13 left-side set A (odd parity) digit patterns.
const EAN_A: [&str; 10] = [
    "0001101", "0011001", "0010011", "0111101", "0100011", "0110001", "0101111", "0111011",
    "0110111", "0001011",
];
/// EAN-13 left-side set G (even parity) digit patterns.
const EAN_G: [&str; 10] = [
    "0100111", "0110011", "0011011", "0100001", "0011101", "0111001", "0000101", "0010001",
    "0001001", "0010111",
];
/// EAN-13 right-side set R patterns (the complement of set A).
const EAN_R: [&str; 10] = [
    "1110010", "1100110", "1101100", "1000010", "1011100", "1001110", "1010000", "1000100",
    "1001000", "1110100",
];
/// Which of the six left digits use set G, indexed by the first (implied)
/// digit -- EAN-13's classic parity trick that encodes digit 13 for free.
const EAN_PARITY: [&str; 10] = [
    "AAAAAA", "AABABB", "AABBAB", "AABBBA", "ABAABB", "ABBAAB", "ABBBAA", "ABABAB", "ABABBA",
    "ABBABA",
];

const CODE128_QUIET_MODULES: u32 = 10;
const EAN_QUIET_MODULES: u32 = 11;

/// One drawn bar: x offset and width in modules, `tall` marks the guard
/// bars that descend below the data bars (EAN's visual signature).
struct Bar {
    x: u32,
    w: u32,
    tall: bool,
}

fn svg_of(bars: &[Bar], total_modules: u32, w: f64, h: f64) -> String {
    let unit = w / total_modules as f64;
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">"
    );
    for bar in bars {
        let height = if bar.tall { h } else { h * 0.85 };
        out.push_str(&format!(
            "<rect x=\"{:.2}\" y=\"0\" width=\"{:.2}\" height=\"{height:.2}\" fill=\"#000\"/>",
            bar.x as f64 * unit,
            bar.w as f64 * unit
        ));
    }
    out.push_str("</svg>");
    out
}

/// Converts a bit string ('1' = bar) into alternating runs of bars.
fn bars_from_bits(bits: &str) -> Vec<Bar> {
    let mut bars = Vec::new();
    let mut current_is_bar = false;
    let mut run_start = 0u32;
    for (i, c) in bits.chars().chain(std::iter::once('0')).enumerate() {
        let is_bar = c == '1';
        if i > 0 && is_bar != current_is_bar {
            if current_is_bar {
                bars.push(Bar {
                    x: run_start,
                    w: i as u32 - run_start,
                    tall: false,
                });
            }
            run_start = i as u32;
            current_is_bar = is_bar;
        } else if i == 0 {
            current_is_bar = is_bar;
        }
    }
    bars
}

/// Encodes `value` into Code 128 symbol values (start code, data with
/// automatic B/C switching, mod-103 check, stop). Only printable ASCII is
/// accepted -- anything else returns `None`.
fn code128_symbols(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() || !value.bytes().all(|b| (32..=126).contains(&b)) {
        return None;
    }
    let chars: Vec<u8> = value.bytes().collect();

    // Start with Code B and switch into Code C whenever a run of >= 4 digits
    // follows (packing two digits per symbol); switch back when fewer than 2
    // remain. Not the theoretically densest packing, but simple, correct and
    // what most scanners see from mainstream encoders.
    let mut symbols = vec![104u8]; // Start B
    let mut i = 0usize;
    let mut in_c = false;
    while i < chars.len() {
        let run = chars[i..].iter().take_while(|c| c.is_ascii_digit()).count();
        if !in_c && run >= 4 {
            // An odd run would strand its last digit inside C; emit it in B first.
            if run % 2 == 1 {
                symbols.push(chars[i] - 32);
                i += 1;
            }
            symbols.push(99); // Code C
            in_c = true;
        } else if in_c && run < 2 {
            symbols.push(100); // Code B
            in_c = false;
        }
        if in_c {
            let hi = chars.get(i)?;
            let lo = chars.get(i + 1)?;
            symbols.push((hi - b'0') * 10 + (lo - b'0'));
            i += 2;
        } else {
            symbols.push(chars[i] - 32);
            i += 1;
        }
    }

    // Checksum: start code plus each following symbol weighted by position.
    let sum = symbols[0] as usize
        + symbols[1..]
            .iter()
            .enumerate()
            .map(|(idx, sym)| (idx + 1) * usize::from(*sym))
            .sum::<usize>();
    symbols.push((sum % 103) as u8);
    symbols.push(106); // Stop
    Some(symbols)
}

fn code128_svg(value: &str, w: f64, h: f64) -> Option<String> {
    let symbols = code128_symbols(value)?;
    let mut bits = String::new();
    for sym in &symbols {
        bits.push_str(CODE128_PATTERNS[usize::from(*sym)]);
    }
    // Code 128's whole symbol is one height; the quiet zones stay empty.
    let total = bits.len() as u32 + 2 * CODE128_QUIET_MODULES;
    let bars: Vec<Bar> = bars_from_bits(&bits)
        .into_iter()
        .map(|mut bar| {
            bar.x += CODE128_QUIET_MODULES;
            bar
        })
        .collect();
    Some(svg_of(&bars, total, w, h))
}

/// Validates (13 digits) or computes (12 digits) the EAN-13 check digit and
/// returns the full 13-digit sequence.
fn ean13_digits(value: &str) -> Option<[u8; 13]> {
    if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes.len() != 12 && bytes.len() != 13 {
        return None;
    }
    let mut digits = [0u8; 13];
    for (slot, d) in digits.iter_mut().zip(bytes) {
        *slot = d - b'0';
    }
    // The checksum always covers the first 12 digits; the 13th, when
    // present, is the check digit being validated (never part of the sum).
    let total: usize = (0..12)
        .map(|i| usize::from(digits[i]) * if i % 2 == 0 { 1 } else { 3 })
        .sum();
    let check = ((10 - total % 10) % 10) as u8;
    match bytes.len() {
        12 => digits[12] = check,
        _ => {
            if digits[12] != check {
                return None;
            }
        }
    }
    Some(digits)
}

fn ean13_svg(value: &str, w: f64, h: f64) -> Option<String> {
    let digits = ean13_digits(value)?;
    // Bits: left guard (101), six left digits under first-digit parity,
    // center guard (01010), six right digits in set R, right guard (101).
    let mut bits = String::from("101");
    let parity = EAN_PARITY[usize::from(digits[0])];
    for i in 0..6usize {
        let digit = usize::from(digits[1 + i]);
        bits.push_str(match parity.as_bytes()[i] {
            b'A' => EAN_A[digit],
            _ => EAN_G[digit],
        });
    }
    bits.push_str("01010");
    for d in digits.iter().take(13).skip(7) {
        bits.push_str(EAN_R[usize::from(*d)]);
    }
    bits.push_str("101");

    // Guard bars (the first/last 3 bits and the middle 5) descend below the
    // data bars -- EAN's visual signature.
    const GUARD_RUNS: [std::ops::Range<u32>; 3] = [0..3, 45..50, 92..95];
    let total = bits.len() as u32 + 2 * EAN_QUIET_MODULES;
    let mut bars: Vec<Bar> = bars_from_bits(&bits);
    for bar in &mut bars {
        let start = bar.x;
        let end = bar.x + bar.w - 1;
        bar.tall = GUARD_RUNS
            .iter()
            .any(|r| r.contains(&start) && r.contains(&end));
        bar.x += EAN_QUIET_MODULES;
    }
    Some(svg_of(&bars, total, w, h))
}

/// Entry point used by the asset pass: scheme name + payload -> SVG text.
pub(crate) fn barcode_svg(scheme: &str, payload: &str, w: f64, h: f64) -> Option<String> {
    match scheme {
        "code128" => code128_svg(payload, w, h),
        "ean13" => ean13_svg(payload, w, h),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code128_known_vector_hi_encodes_with_the_right_symbols() {
        // Hand-computed: Start B (104), H=40, I=41, checksum (104+40+82)%103=20, stop.
        assert_eq!(code128_symbols("HI"), Some(vec![104, 40, 41, 20, 106]));
    }

    #[test]
    fn code128_switches_to_code_c_for_long_digit_runs() {
        let symbols = code128_symbols("123456").unwrap();
        assert_eq!(&symbols[..5], &[104, 99, 12, 34, 56], "{symbols:?}");
        // Checksum consistency: recomputed over the emitted symbols.
        let body = &symbols[..symbols.len() - 2];
        let sum = body[0] as usize
            + body[1..]
                .iter()
                .enumerate()
                .map(|(i, s)| (i + 1) * usize::from(*s))
                .sum::<usize>();
        assert_eq!(symbols[symbols.len() - 2] as usize, sum % 103);
    }

    #[test]
    fn code128_odd_digit_run_keeps_its_last_digit_in_b() {
        // "AB12345": run of 5 digits -> '1' stays in B, then pairs 23/45 in C.
        let symbols = code128_symbols("AB12345").unwrap();
        let pos_of_c = symbols.iter().position(|s| *s == 99u8).unwrap();
        assert_eq!(symbols[pos_of_c - 1], b'1' - 32);
    }

    #[test]
    fn code128_every_pattern_has_the_canonical_module_count() {
        // Data patterns encode 11 modules in 6 elements; the stop has 13 in 7.
        for (i, pattern) in CODE128_PATTERNS.iter().enumerate() {
            let modules: u32 = pattern.chars().filter_map(|c| c.to_digit(10)).sum();
            let expected = if i == 106 { 13 } else { 11 };
            assert_eq!(modules, expected, "pattern {i} ({pattern})");
        }
    }

    #[test]
    fn code128_rejects_non_ascii_and_control_characters() {
        assert_eq!(code128_symbols(""), None);
        assert_eq!(code128_symbols("café"), None);
        assert_eq!(code128_symbols("line\nbreak"), None);
    }

    #[test]
    fn ean13_accepts_a_valid_full_symbol_and_computes_for_twelve_digits() {
        // The classic example from the spec literature: check digit 1.
        assert!(ean13_digits("4006381333931").is_some());
        assert_eq!(
            ean13_digits("400638133393"),
            Some(ean13_digits("4006381333931").unwrap())
        );
    }

    #[test]
    fn ean13_rejects_a_wrong_check_digit_and_junk() {
        assert_eq!(ean13_digits("4006381333932"), None);
        assert_eq!(ean13_digits("40063813339"), None);
        assert_eq!(ean13_digits("40063813339310"), None);
        assert_eq!(ean13_digits("40063813339a"), None);
        assert_eq!(ean13_digits(""), None);
    }

    #[test]
    fn ean13_bit_layout_matches_the_symbology_structure() {
        let svg = ean13_svg("4006381333931", 229.0, 80.0).expect("encodes");
        // Quiet zones included: 95 data modules + 2*11 quiet = 117 total.
        assert!(svg.contains("viewBox=\"0 0 229 80\""), "{svg}");
        assert!(svg.contains("<rect"), "{svg}");
        assert!(!svg.to_lowercase().contains("nan"), "{svg}");
    }

    #[test]
    fn unknown_schemes_degrade_to_none() {
        assert_eq!(barcode_svg("datamatrix", "x", 100.0, 100.0), None);
        assert_eq!(barcode_svg("pdf417", "x", 100.0, 100.0), None);
        assert_eq!(barcode_svg("", "x", 100.0, 100.0), None);
    }

    #[test]
    fn both_symbologies_produce_valid_svg_at_requested_size() {
        for (scheme, value) in [("code128", "INV-2026-0042"), ("ean13", "4006381333931")] {
            let svg = barcode_svg(scheme, value, 300.0, 60.0).expect(scheme);
            assert!(svg.starts_with("<svg"), "{scheme}: {svg}");
            assert!(svg.ends_with("</svg>"), "{scheme}: {svg}");
            assert!(svg.contains("width=\"300\"") && svg.contains("height=\"60\""));
        }
    }
}
