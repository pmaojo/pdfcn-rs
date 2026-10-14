//! `%VerifactuQR`: the QR code a Spanish invoice must carry under
//! VERI*FACTU (Real Decreto 1007/2023), built from the invoice's own data.
//!
//! The QR encodes a URL on the tax agency's (AEAT) verification service with
//! four query parameters: the issuer's NIF, the series + number, the issue
//! date as `dd-mm-yyyy`, and the total amount with a decimal point. Scanning
//! it lets the customer check the invoice against what the issuer reported.
//!
//! The component validates all four values and renders a visible error chip
//! instead of a QR code that would send the customer to a page saying the
//! invoice does not exist. The symbol itself (error-correction level M, quiet
//! zone) is produced by `pdfcn-core`'s existing `pdfcn-qrcode:` pipeline.
//!
//! `importe` must be the invoice's "Importe total" (taxable base plus VAT and
//! any equivalence surcharge), not the amount to pay: withholdings and suplidos
//! are not part of the invoice record the AEAT compares the QR against.
//!
//! What this does *not* do: generate the invoice record, its hash chain or
//! the submission to the AEAT. It prints the QR for an invoice that your
//! billing system has registered.

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;

use crate::{attr, invalid_component};

const PRODUCTION: &str = "https://www2.agenciatributaria.gob.es/wlpl/TIKE-CONT";
const TEST: &str = "https://prewww2.aeat.es/wlpl/TIKE-CONT";

/// Whether the issuer's system sends every invoice to the AEAT as it is
/// issued (VERI*FACTU) or only keeps the records and can send them on request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Verifactu,
    NoVerifactu,
}

/// Which AEAT service the QR points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Production,
    Test,
}

/// Builds the verification URL, or says exactly what is wrong with the input.
pub fn qr_url(
    nif: &str,
    numserie: &str,
    fecha: &str,
    importe: &str,
    mode: Mode,
    environment: Environment,
) -> Result<String, String> {
    let nif = nif.trim().to_ascii_uppercase();
    if nif.len() != 9
        || !nif
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
    {
        return Err(format!("nif must be 9 letters/digits, got \"{nif}\""));
    }
    if numserie.is_empty() || numserie.len() > 60 {
        return Err(format!(
            "numserie must be 1 to 60 characters, got {}",
            numserie.len()
        ));
    }
    if !numserie.bytes().all(|b| (32..=126).contains(&b)) {
        return Err("numserie may only contain printable ASCII".into());
    }
    let fecha = normalise_date(fecha)?;
    let importe = normalise_amount(importe)?;

    let base = match environment {
        Environment::Production => PRODUCTION,
        Environment::Test => TEST,
    };
    let endpoint = match mode {
        Mode::Verifactu => "ValidarQR",
        Mode::NoVerifactu => "ValidarQRNoVerifactu",
    };
    Ok(format!(
        "{base}/{endpoint}?nif={}&numserie={}&fecha={fecha}&importe={importe}",
        percent_encode(&nif),
        percent_encode(numserie),
    ))
}

/// RFC 3986 unreserved characters pass; everything else, a space or a slash
/// included, becomes `%XX`.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Accepts `dd-mm-yyyy` (what the AEAT wants) and `yyyy-mm-dd` (what JSON
/// data usually carries), and checks the day exists.
fn normalise_date(text: &str) -> Result<String, String> {
    let parts: Vec<&str> = text.trim().split('-').collect();
    let numbers: Option<Vec<u32>> = parts.iter().map(|p| p.parse().ok()).collect();
    let (d, m, y) = match (numbers.as_deref(), parts.as_slice()) {
        (Some([d, m, y]), [a, _, c]) if a.len() <= 2 && c.len() == 4 => (*d, *m, *y),
        (Some([y, m, d]), [a, _, c]) if a.len() == 4 && c.len() <= 2 => (*d, *m, *y),
        _ => {
            return Err(format!(
                "fecha must be dd-mm-yyyy or yyyy-mm-dd, got \"{text}\""
            ))
        }
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(format!("fecha has no month {m}")),
    };
    if d == 0 || d > days {
        return Err(format!("fecha \"{text}\" is not a real date"));
    }
    Ok(format!("{d:02}-{m:02}-{y:04}"))
}

/// A plain decimal with a point (or a single decimal comma), at most 12
/// integer digits and 2 decimals. Currency symbols and thousands separators
/// are errors: guessing would print a QR for the wrong amount.
fn normalise_amount(text: &str) -> Result<String, String> {
    let text = text.trim();
    let text = if text.matches(',').count() == 1 && !text.contains('.') {
        text.replace(',', ".")
    } else {
        text.to_string()
    };
    let unsigned = text.strip_prefix('-').unwrap_or(&text);
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty()
        || !digits(whole)
        || !digits(fraction)
        || whole.len() > 12
        || fraction.len() > 2
        || (unsigned.contains('.') && fraction.is_empty())
    {
        return Err(format!(
            "importe must be a plain number with up to 12 digits and 2 decimals (no currency symbol or thousands separator), got \"{text}\""
        ));
    }
    Ok(text)
}

pub fn verifactu_qr(attrs: &[ResolvedAttr]) -> Markup {
    let get = |name: &str| attr(attrs, name).unwrap_or("");
    let mode = match get("mode") {
        "" | "verifactu" => Mode::Verifactu,
        "no-verifactu" => Mode::NoVerifactu,
        other => {
            return error(&format!(
                "mode must be \"verifactu\" or \"no-verifactu\", got \"{other}\""
            ))
        }
    };
    let environment = match get("env") {
        "" | "production" => Environment::Production,
        "test" => Environment::Test,
        other => {
            return error(&format!(
                "env must be \"production\" or \"test\", got \"{other}\""
            ))
        }
    };
    let url = match qr_url(
        get("nif"),
        get("numserie"),
        get("fecha"),
        get("importe"),
        mode,
        environment,
    ) {
        Ok(url) => url,
        Err(message) => return error(&message),
    };
    // 150 px is 39.7 mm at the engine's 96 px/in: the whole symbol, quiet zone
    // included, stays inside the 30-40 mm the AEAT asks for, and the modules
    // alone (about 34 mm) stay above its 30 mm floor.
    let size = attr(attrs, "size").unwrap_or("150px");
    html! {
        div class="inline-block text-center" {
            @if mode == Mode::Verifactu {
                p class="text-xs font-semibold" { "VERI*FACTU" }
            }
            img class="qrcode inline-block" src=(format!("pdfcn-qrcode:{}", hex(url.as_bytes()))) style={ "width:" (size) ";height:" (size) ";" } alt="QR code to verify the invoice with the tax agency";
        }
    }
}

fn error(message: &str) -> Markup {
    html! {
        div class=(invalid_component()) { "VerifactuQR: " (message) }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(numserie: &str, fecha: &str, importe: &str) -> Result<String, String> {
        qr_url(
            "B43752210",
            numserie,
            fecha,
            importe,
            Mode::Verifactu,
            Environment::Test,
        )
    }

    #[test]
    fn builds_the_url_the_agency_documents() {
        assert_eq!(
            url("E-12345", "23-04-2025", "123.45").unwrap(),
            "https://prewww2.aeat.es/wlpl/TIKE-CONT/ValidarQR?nif=B43752210&numserie=E-12345&fecha=23-04-2025&importe=123.45"
        );
    }

    #[test]
    fn production_and_no_verifactu_use_their_own_endpoints() {
        let production = qr_url(
            "B43752210",
            "1",
            "01-01-2026",
            "1",
            Mode::Verifactu,
            Environment::Production,
        )
        .unwrap();
        assert!(production
            .starts_with("https://www2.agenciatributaria.gob.es/wlpl/TIKE-CONT/ValidarQR?"));
        let other = qr_url(
            "B43752210",
            "1",
            "01-01-2026",
            "1",
            Mode::NoVerifactu,
            Environment::Production,
        )
        .unwrap();
        assert!(other.contains("/ValidarQRNoVerifactu?"), "{other}");
    }

    #[test]
    fn the_series_is_percent_encoded() {
        assert!(url("1/25A-321", "23-04-2025", "1")
            .unwrap()
            .contains("numserie=1%2F25A-321&"));
        assert!(url("A 1&b=2", "23-04-2025", "1")
            .unwrap()
            .contains("numserie=A%201%26b%3D2&"));
    }

    #[test]
    fn dates_are_accepted_as_iso_and_checked() {
        assert!(url("1", "2025-04-23", "1")
            .unwrap()
            .contains("fecha=23-04-2025"));
        assert!(url("1", "2024-02-29", "1").is_ok());
        assert!(url("1", "2025-02-29", "1").is_err());
        assert!(url("1", "31-04-2025", "1").is_err());
        assert!(url("1", "23/04/2025", "1").is_err());
    }

    #[test]
    fn amounts_are_strict() {
        assert!(url("1", "01-01-2026", "194.40")
            .unwrap()
            .ends_with("importe=194.40"));
        assert!(url("1", "01-01-2026", "194,40")
            .unwrap()
            .ends_with("importe=194.40"));
        assert!(url("1", "01-01-2026", "-50")
            .unwrap()
            .ends_with("importe=-50"));
        for bad in [
            "$194.40", "1.194,40", "1,194.40", "12.345", "", "1.", ".5", "1e3",
        ] {
            assert!(
                url("1", "01-01-2026", bad).is_err(),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn the_nif_is_checked_and_uppercased() {
        assert!(qr_url(
            "b43752210",
            "1",
            "01-01-2026",
            "1",
            Mode::Verifactu,
            Environment::Test
        )
        .unwrap()
        .contains("nif=B43752210"));
        assert!(qr_url(
            "B4375221",
            "1",
            "01-01-2026",
            "1",
            Mode::Verifactu,
            Environment::Test
        )
        .is_err());
        assert!(qr_url(
            "B-3752210",
            "1",
            "01-01-2026",
            "1",
            Mode::Verifactu,
            Environment::Test
        )
        .is_err());
    }

    #[test]
    fn the_series_length_and_charset_are_checked() {
        assert!(url("", "01-01-2026", "1").is_err());
        assert!(url(&"A".repeat(61), "01-01-2026", "1").is_err());
        assert!(url(&"A".repeat(60), "01-01-2026", "1").is_ok());
        assert!(url("Nº1", "01-01-2026", "1").is_err());
    }

    fn a(name: &str, value: &str) -> ResolvedAttr {
        ResolvedAttr {
            name: name.into(),
            value: value.into(),
        }
    }

    fn attrs() -> Vec<ResolvedAttr> {
        vec![
            a("nif", "B43752210"),
            a("numserie", "E-12345"),
            a("fecha", "2025-04-23"),
            a("importe", "123.45"),
        ]
    }

    #[test]
    fn the_component_prints_the_label_and_a_qr_placeholder() {
        let out = verifactu_qr(&attrs()).into_string();
        assert!(out.contains("VERI*FACTU"), "{out}");
        assert!(out.contains("src=\"pdfcn-qrcode:"), "{out}");
    }

    #[test]
    fn no_verifactu_mode_has_no_label() {
        let mut attrs = attrs();
        attrs.push(a("mode", "no-verifactu"));
        let out = verifactu_qr(&attrs).into_string();
        assert!(!out.contains("VERI*FACTU"), "{out}");
    }

    #[test]
    fn bad_data_is_a_visible_error_never_a_wrong_qr() {
        let mut attrs = attrs();
        attrs[3] = a("importe", "$123.45");
        let out = verifactu_qr(&attrs).into_string();
        assert!(out.contains("pdfcn-invalid-component"), "{out}");
        assert!(out.contains("importe"), "{out}");
        assert!(!out.contains("pdfcn-qrcode:"), "{out}");
    }
}
