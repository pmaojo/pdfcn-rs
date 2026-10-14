//! `%FiscalQR`: the QR code a tax authority asks for on an invoice, driven by a
//! *profile* instead of code.
//!
//! Every country that wants a QR on invoices wants a different one: a URL on
//! the tax agency's verification service in Spain, a string of `key:value`
//! pairs in others, a different set of fields, a different date and amount
//! format. Hard-coding each would make pdfcn a catalogue of other people's
//! regulations, always one amendment behind. A profile is a small JSON file
//! that says what the specification says:
//!
//! ```json
//! {
//!   "id": "xx-demo",
//!   "label": "SCAN TO VERIFY",
//!   "ecc": "M",
//!   "fields": {
//!     "tax_id": { "type": "text", "charset": "alnum", "length": 9, "uppercase": true },
//!     "number": { "type": "text", "min": 1, "max": 30, "encode": "url" },
//!     "date":   { "type": "date", "input": ["yyyy-mm-dd"], "output": "dd-mm-yyyy" },
//!     "total":  { "type": "amount", "max_decimals": 2, "fixed_decimals": 2 }
//!   },
//!   "payload": "{base}?id={tax_id}&n={number}&d={date}&t={total}",
//!   "variants": { "env": {
//!     "default": "production",
//!     "options": {
//!       "production": { "vars": { "base": "https://verify.example/qr" } },
//!       "test": { "vars": { "base": "https://test.verify.example/qr" } }
//!     }
//!   } }
//! }
//! ```
//!
//! used as `%FiscalQR(profile="xx-demo" tax_id="..." number="..." date="..." total="..." env="test")`.
//!
//! Spain's VERI*FACTU is one such profile, shipped as data (see
//! `profiles/es-verifactu.json`) and reachable through the `%VerifactuQR`
//! shorthand. The engine validates every value against its field rules and
//! renders a visible error chip when one is wrong, never a QR that would send
//! the customer to a page saying the invoice does not exist.
//!
//! A profile describes the *printed code* only. The invoice record, any hash
//! chain, signing and submission to the authority are the billing system's
//! job, not a renderer's.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{OnceLock, RwLock};

use maud::{html, Markup};
use pdfcn_template::ResolvedAttr;
use serde_json::Value;

use crate::{attr, invalid_component};

const BUILTIN: &[&str] = &[include_str!("profiles/es-verifactu.json")];

/// Attribute names the component itself uses; a field or variant may not
/// shadow them.
const RESERVED: &[&str] = &["profile", "size"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Charset {
    Any,
    Alnum,
    Digits,
    PrintableAscii,
}

#[derive(Debug, Clone)]
struct TextRules {
    charset: Charset,
    min: usize,
    max: usize,
    uppercase: bool,
    trim: bool,
    allowed: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
struct AmountRules {
    max_integer_digits: usize,
    max_decimals: usize,
    fixed_decimals: Option<usize>,
    allow_negative: bool,
    decimal_comma: bool,
    separator: char,
}

#[derive(Debug, Clone)]
enum Kind {
    Text(TextRules),
    Date {
        input: Vec<Vec<DateToken>>,
        output: Vec<DateToken>,
    },
    Amount(AmountRules),
}

#[derive(Debug, Clone)]
struct Field {
    kind: Kind,
    url_encode: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DateToken {
    Day,
    Month,
    Year,
    Literal(char),
}

#[derive(Debug, Clone)]
enum Piece {
    Literal(String),
    Reference(String),
}

#[derive(Debug, Clone)]
struct VariantOption {
    name: String,
    vars: BTreeMap<String, String>,
    /// `Some(None)` removes the label, `Some(Some(_))` replaces it.
    label: Option<Option<String>>,
}

#[derive(Debug, Clone)]
struct Dimension {
    name: String,
    /// Index into `options`. Named explicitly in the profile: JSON objects
    /// carry no order a profile could rely on.
    default: usize,
    options: Vec<VariantOption>,
}

/// A parsed, validated profile.
#[derive(Debug, Clone)]
pub struct Profile {
    id: String,
    label: Option<String>,
    size: String,
    ecc: char,
    fields: Vec<(String, Field)>,
    payload: Vec<Piece>,
    dimensions: Vec<Dimension>,
}

type Registry = RwLock<BTreeMap<String, Profile>>;

fn registry() -> &'static Registry {
    static REGISTRY: OnceLock<Registry> = OnceLock::new();
    REGISTRY.get_or_init(|| {
        let mut map = BTreeMap::new();
        for json in BUILTIN {
            // A built-in that does not parse is a bug in this crate; the unit
            // tests parse every one, so this cannot ship broken.
            if let Ok(profile) = Profile::parse(json) {
                map.insert(profile.id.clone(), profile);
            }
        }
        RwLock::new(map)
    })
}

/// Registers a profile from its JSON text and returns its id. A profile with
/// an id that is already registered replaces it, so a project can pin its own
/// reading of a specification (including over a built-in one).
pub fn register_fiscal_profile(json: &str) -> Result<String, String> {
    let profile = Profile::parse(json)?;
    let id = profile.id.clone();
    registry()
        .write()
        .map_err(|_| "profile registry is poisoned".to_string())?
        .insert(id.clone(), profile);
    Ok(id)
}

/// The ids of every registered profile.
pub fn fiscal_profile_ids() -> Vec<String> {
    registry()
        .read()
        .map(|r| r.keys().cloned().collect())
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

fn get_str<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn get_usize(v: &Value, key: &str) -> Result<Option<usize>, String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(n) => n
            .as_u64()
            .map(|n| Some(n as usize))
            .ok_or_else(|| format!("\"{key}\" must be a non-negative integer")),
    }
}

fn get_bool(v: &Value, key: &str, default: bool) -> Result<bool, String> {
    match v.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(b) => b
            .as_bool()
            .ok_or_else(|| format!("\"{key}\" must be true or false")),
    }
}

/// Rejects keys a profile misspelt: a typo in a validation rule would
/// otherwise silently turn the rule off.
fn only_keys(v: &Value, allowed: &[&str], what: &str) -> Result<(), String> {
    let Some(object) = v.as_object() else {
        return Err(format!("{what} must be an object"));
    };
    for key in object.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "{what} has an unknown key \"{key}\" (expected one of: {})",
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

fn parse_date_format(text: &str) -> Result<Vec<DateToken>, String> {
    let mut tokens = Vec::new();
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(r) = rest.strip_prefix("yyyy") {
            tokens.push(DateToken::Year);
            rest = r;
        } else if let Some(r) = rest.strip_prefix("dd") {
            tokens.push(DateToken::Day);
            rest = r;
        } else if let Some(r) = rest.strip_prefix("mm") {
            tokens.push(DateToken::Month);
            rest = r;
        } else {
            let c = rest.chars().next().unwrap_or(' ');
            if c.is_ascii_alphanumeric() {
                return Err(format!(
                    "date format \"{text}\": unexpected \"{c}\" (use dd, mm, yyyy and separators)"
                ));
            }
            tokens.push(DateToken::Literal(c));
            rest = &rest[c.len_utf8()..];
        }
    }
    let has = |t: &DateToken| tokens.contains(t);
    if !(has(&DateToken::Day) && has(&DateToken::Month) && has(&DateToken::Year)) {
        return Err(format!("date format \"{text}\" needs dd, mm and yyyy"));
    }
    Ok(tokens)
}

fn parse_field(name: &str, spec: &Value) -> Result<Field, String> {
    let what = format!("field \"{name}\"");
    let kind = get_str(spec, "type").ok_or_else(|| format!("{what} needs a \"type\""))?;
    let url_encode = match get_str(spec, "encode") {
        None | Some("none") => false,
        Some("url") => true,
        Some(other) => {
            return Err(format!(
                "{what}: encode must be \"url\" or \"none\", got \"{other}\""
            ))
        }
    };
    let kind = match kind {
        "text" => {
            only_keys(
                spec,
                &[
                    "type",
                    "encode",
                    "charset",
                    "length",
                    "min",
                    "max",
                    "uppercase",
                    "trim",
                    "allowed",
                ],
                &what,
            )?;
            let charset = match get_str(spec, "charset").unwrap_or("any") {
                "any" => Charset::Any,
                "alnum" => Charset::Alnum,
                "digits" => Charset::Digits,
                "printable_ascii" => Charset::PrintableAscii,
                other => {
                    return Err(format!(
                        "{what}: charset must be any, alnum, digits or printable_ascii, got \"{other}\""
                    ))
                }
            };
            let exact = get_usize(spec, "length").map_err(|e| format!("{what}: {e}"))?;
            let min = get_usize(spec, "min").map_err(|e| format!("{what}: {e}"))?;
            let max = get_usize(spec, "max").map_err(|e| format!("{what}: {e}"))?;
            let (min, max) = match exact {
                Some(n) => (n, n),
                None => (min.unwrap_or(1), max.unwrap_or(usize::MAX)),
            };
            if min > max {
                return Err(format!("{what}: min is larger than max"));
            }
            let allowed = match spec.get("allowed") {
                None | Some(Value::Null) => None,
                Some(Value::Array(items)) => Some(
                    items
                        .iter()
                        .map(|i| {
                            i.as_str()
                                .map(str::to_string)
                                .ok_or_else(|| format!("{what}: allowed must be a list of strings"))
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                ),
                Some(_) => return Err(format!("{what}: allowed must be a list of strings")),
            };
            Kind::Text(TextRules {
                charset,
                min,
                max,
                uppercase: get_bool(spec, "uppercase", false)
                    .map_err(|e| format!("{what}: {e}"))?,
                trim: get_bool(spec, "trim", true).map_err(|e| format!("{what}: {e}"))?,
                allowed,
            })
        }
        "date" => {
            only_keys(spec, &["type", "encode", "input", "output"], &what)?;
            let output = parse_date_format(
                get_str(spec, "output")
                    .ok_or_else(|| format!("{what} needs an \"output\" format"))?,
            )?;
            let input =
                match spec.get("input") {
                    None => vec![output.clone()],
                    Some(Value::Array(items)) if !items.is_empty() => items
                        .iter()
                        .map(|i| {
                            parse_date_format(i.as_str().ok_or_else(|| {
                                format!("{what}: input must be a list of strings")
                            })?)
                        })
                        .collect::<Result<Vec<_>, _>>()?,
                    Some(_) => {
                        return Err(format!("{what}: input must be a non-empty list of strings"))
                    }
                };
            Kind::Date { input, output }
        }
        "amount" => {
            only_keys(
                spec,
                &[
                    "type",
                    "encode",
                    "max_integer_digits",
                    "max_decimals",
                    "fixed_decimals",
                    "allow_negative",
                    "decimal_comma",
                    "separator",
                ],
                &what,
            )?;
            let separator = match get_str(spec, "separator").unwrap_or(".") {
                "." => '.',
                "," => ',',
                other => {
                    return Err(format!(
                        "{what}: separator must be \".\" or \",\", got \"{other}\""
                    ))
                }
            };
            let max_decimals = get_usize(spec, "max_decimals")
                .map_err(|e| format!("{what}: {e}"))?
                .unwrap_or(2);
            let fixed_decimals =
                get_usize(spec, "fixed_decimals").map_err(|e| format!("{what}: {e}"))?;
            if fixed_decimals.is_some_and(|f| f > max_decimals) {
                return Err(format!(
                    "{what}: fixed_decimals is larger than max_decimals"
                ));
            }
            Kind::Amount(AmountRules {
                max_integer_digits: get_usize(spec, "max_integer_digits")
                    .map_err(|e| format!("{what}: {e}"))?
                    .unwrap_or(15),
                max_decimals,
                fixed_decimals,
                allow_negative: get_bool(spec, "allow_negative", false)
                    .map_err(|e| format!("{what}: {e}"))?,
                decimal_comma: get_bool(spec, "decimal_comma", false)
                    .map_err(|e| format!("{what}: {e}"))?,
                separator,
            })
        }
        other => {
            return Err(format!(
                "{what}: type must be text, date or amount, got \"{other}\""
            ))
        }
    };
    Ok(Field { kind, url_encode })
}

fn parse_payload(template: &str) -> Result<Vec<Piece>, String> {
    let mut pieces = Vec::new();
    let mut literal = String::new();
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                literal.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                literal.push('}');
            }
            '{' => {
                let mut name = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(c) => name.push(c),
                        None => return Err("payload has an unclosed \"{\"".into()),
                    }
                }
                if name.is_empty() {
                    return Err("payload has an empty \"{}\"".into());
                }
                if !literal.is_empty() {
                    pieces.push(Piece::Literal(std::mem::take(&mut literal)));
                }
                pieces.push(Piece::Reference(name));
            }
            '}' => {
                return Err("payload has a stray \"}\" (write \"}}\" for a literal brace)".into())
            }
            c => literal.push(c),
        }
    }
    if !literal.is_empty() {
        pieces.push(Piece::Literal(literal));
    }
    Ok(pieces)
}

fn parse_dimension(name: &str, spec: &Value) -> Result<Dimension, String> {
    let what = format!("variant \"{name}\"");
    only_keys(spec, &["default", "options"], &what)?;
    let Some(options) = spec.get("options").and_then(Value::as_object) else {
        return Err(format!("{what} needs an \"options\" object"));
    };
    if options.is_empty() {
        return Err(format!("{what} needs at least one option"));
    }
    let mut parsed = Vec::new();
    for (option, body) in options {
        only_keys(
            body,
            &["vars", "label"],
            &format!("{what} option \"{option}\""),
        )?;
        let mut vars = BTreeMap::new();
        if let Some(v) = body.get("vars") {
            let Some(map) = v.as_object() else {
                return Err(format!(
                    "{what} option \"{option}\": vars must be an object"
                ));
            };
            for (k, val) in map {
                let text = val.as_str().ok_or_else(|| {
                    format!("{what} option \"{option}\": var \"{k}\" must be a string")
                })?;
                vars.insert(k.clone(), text.to_string());
            }
        }
        let label = match body.get("label") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(Value::String(s)) => Some(Some(s.clone())),
            Some(_) => {
                return Err(format!(
                    "{what} option \"{option}\": label must be a string or null"
                ))
            }
        };
        parsed.push(VariantOption {
            name: option.clone(),
            vars,
            label,
        });
    }
    let wanted = get_str(spec, "default").ok_or_else(|| {
        format!(
            "{what} needs a \"default\" naming one of: {}",
            parsed
                .iter()
                .map(|o| o.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    })?;
    let default = parsed
        .iter()
        .position(|o| o.name == wanted)
        .ok_or_else(|| format!("{what}: default \"{wanted}\" is not one of its options"))?;
    Ok(Dimension {
        name: name.to_string(),
        default,
        options: parsed,
    })
}

impl Profile {
    pub fn parse(json: &str) -> Result<Profile, String> {
        let root: Value =
            serde_json::from_str(json).map_err(|e| format!("profile is not valid JSON: {e}"))?;
        only_keys(
            &root,
            &[
                "id",
                "description",
                "label",
                "size",
                "ecc",
                "fields",
                "payload",
                "variants",
            ],
            "profile",
        )?;
        let id = get_str(&root, "id").ok_or("profile needs an \"id\"")?;
        if id.is_empty()
            || !id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(format!(
                "profile id \"{id}\" must be lowercase letters, digits, \"-\" or \"_\" (e.g. \"es-verifactu\")"
            ));
        }
        let ecc = match get_str(&root, "ecc").unwrap_or("M") {
            "L" => 'L',
            "M" => 'M',
            "Q" => 'Q',
            "H" => 'H',
            other => return Err(format!("ecc must be L, M, Q or H, got \"{other}\"")),
        };

        let mut fields = Vec::new();
        let Some(field_map) = root.get("fields").and_then(Value::as_object) else {
            return Err("profile needs a \"fields\" object".into());
        };
        for (name, spec) in field_map {
            if RESERVED.contains(&name.as_str()) {
                return Err(format!(
                    "field \"{name}\" clashes with a reserved attribute"
                ));
            }
            fields.push((name.clone(), parse_field(name, spec)?));
        }

        let mut dimensions = Vec::new();
        if let Some(variants) = root.get("variants") {
            let Some(map) = variants.as_object() else {
                return Err("\"variants\" must be an object".into());
            };
            for (name, spec) in map {
                if RESERVED.contains(&name.as_str()) || field_map.contains_key(name) {
                    return Err(format!(
                        "variant \"{name}\" clashes with a field or reserved attribute"
                    ));
                }
                dimensions.push(parse_dimension(name, spec)?);
            }
        }

        let payload = parse_payload(
            get_str(&root, "payload").ok_or("profile needs a \"payload\" template")?,
        )?;

        // Every placeholder must be a field or a variable that every option of
        // some variant defines; every field must be used. Catching both here
        // means a typo fails at registration, not on the first printed invoice.
        let mut variables = BTreeSet::new();
        for dim in &dimensions {
            let mut common: Option<BTreeSet<&String>> = None;
            for option in &dim.options {
                let keys: BTreeSet<&String> = option.vars.keys().collect();
                common = Some(match common {
                    None => keys,
                    Some(c) => c.intersection(&keys).copied().collect(),
                });
            }
            for var in common.unwrap_or_default() {
                variables.insert(var.clone());
            }
        }
        let mut used = BTreeSet::new();
        for piece in &payload {
            if let Piece::Reference(name) = piece {
                if field_map.contains_key(name) {
                    used.insert(name.clone());
                } else if !variables.contains(name) {
                    return Err(format!(
                        "payload uses {{{name}}}, which is neither a field nor a variable every option of a variant defines"
                    ));
                }
            }
        }
        for (name, _) in &fields {
            if !used.contains(name) {
                return Err(format!(
                    "field \"{name}\" is declared but never used in the payload"
                ));
            }
        }

        Ok(Profile {
            id: id.to_string(),
            label: get_str(&root, "label").map(str::to_string),
            size: get_str(&root, "size").unwrap_or("150px").to_string(),
            ecc,
            fields,
            payload,
            dimensions,
        })
    }
}

// ---------------------------------------------------------------------------
// Validation and rendering
// ---------------------------------------------------------------------------

/// RFC 3986 unreserved characters pass; everything else becomes `%XX`.
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

fn check_text(name: &str, value: &str, rules: &TextRules) -> Result<String, String> {
    let mut text = if rules.trim { value.trim() } else { value }.to_string();
    if rules.uppercase {
        text = text.to_uppercase();
    }
    let length = text.chars().count();
    if length < rules.min || length > rules.max {
        let want = if rules.min == rules.max {
            format!("exactly {}", rules.min)
        } else if rules.max == usize::MAX {
            format!("at least {}", rules.min)
        } else {
            format!("{} to {}", rules.min, rules.max)
        };
        return Err(format!("{name} must be {want} characters, got {length}"));
    }
    let ok = match rules.charset {
        Charset::Any => true,
        Charset::Alnum => text.chars().all(|c| c.is_ascii_alphanumeric()),
        Charset::Digits => text.chars().all(|c| c.is_ascii_digit()),
        Charset::PrintableAscii => text.bytes().all(|b| (32..=126).contains(&b)),
    };
    if !ok {
        let what = match rules.charset {
            Charset::Alnum => "letters and digits only",
            Charset::Digits => "digits only",
            _ => "printable ASCII only",
        };
        return Err(format!("{name} must be {what}, got \"{text}\""));
    }
    if let Some(list) = &rules.allowed {
        if !list.contains(&text) {
            return Err(format!(
                "{name} must be one of {}, got \"{text}\"",
                list.join(", ")
            ));
        }
    }
    Ok(text)
}

/// Reads `text` according to `format`; returns day, month, year.
fn parse_date(text: &str, format: &[DateToken]) -> Option<(u32, u32, u32)> {
    let chars: Vec<char> = text.trim().chars().collect();
    let mut at = 0;
    let (mut day, mut month, mut year) = (None, None, None);
    for (i, token) in format.iter().enumerate() {
        match token {
            DateToken::Literal(c) => {
                if chars.get(at) != Some(c) {
                    return None;
                }
                at += 1;
            }
            _ => {
                let width = if *token == DateToken::Year { 4 } else { 2 };
                // Before a separator, one or two digits are fine ("3-4-2026").
                let flexible = matches!(format.get(i + 1), Some(DateToken::Literal(_)));
                let mut digits = String::new();
                while digits.len() < width && chars.get(at).is_some_and(char::is_ascii_digit) {
                    digits.push(chars[at]);
                    at += 1;
                }
                let long_enough = if flexible && *token != DateToken::Year {
                    !digits.is_empty()
                } else {
                    digits.len() == width
                };
                if !long_enough {
                    return None;
                }
                let n: u32 = digits.parse().ok()?;
                match token {
                    DateToken::Day => day = Some(n),
                    DateToken::Month => month = Some(n),
                    _ => year = Some(n),
                }
            }
        }
    }
    if at != chars.len() {
        return None;
    }
    Some((day?, month?, year?))
}

fn check_date(
    name: &str,
    value: &str,
    input: &[Vec<DateToken>],
    output: &[DateToken],
) -> Result<String, String> {
    let Some((d, m, y)) = input.iter().find_map(|f| parse_date(value, f)) else {
        return Err(format!(
            "{name} \"{value}\" is not in an accepted date format"
        ));
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err(format!("{name} \"{value}\" has no month {m}")),
    };
    if d == 0 || d > days {
        return Err(format!("{name} \"{value}\" is not a real date"));
    }
    Ok(output
        .iter()
        .map(|t| match t {
            DateToken::Day => format!("{d:02}"),
            DateToken::Month => format!("{m:02}"),
            DateToken::Year => format!("{y:04}"),
            DateToken::Literal(c) => c.to_string(),
        })
        .collect())
}

fn check_amount(name: &str, value: &str, rules: &AmountRules) -> Result<String, String> {
    let text = value.trim();
    let text = if rules.decimal_comma && text.matches(',').count() == 1 && !text.contains('.') {
        text.replace(',', ".")
    } else {
        text.to_string()
    };
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.as_str()),
    };
    let (whole, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    let digits = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    let bad = whole.is_empty()
        || !digits(whole)
        || !digits(fraction)
        || whole.len() > rules.max_integer_digits
        || fraction.len() > rules.max_decimals
        || (unsigned.contains('.') && fraction.is_empty())
        || (negative && !rules.allow_negative);
    if bad {
        return Err(format!(
            "{name} must be a plain number with up to {} digits and {} decimals{} (no currency symbol or thousands separator), got \"{text}\"",
            rules.max_integer_digits,
            rules.max_decimals,
            if rules.allow_negative { "" } else { ", not negative" }
        ));
    }
    let mut fraction = fraction.to_string();
    if let Some(fixed) = rules.fixed_decimals {
        while fraction.len() < fixed {
            fraction.push('0');
        }
    }
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    out.push_str(whole);
    if !fraction.is_empty() {
        out.push(rules.separator);
        out.push_str(&fraction);
    }
    Ok(out)
}

fn check_field(name: &str, field: &Field, value: &str) -> Result<String, String> {
    let checked = match &field.kind {
        Kind::Text(rules) => check_text(name, value, rules)?,
        Kind::Date { input, output } => check_date(name, value, input, output)?,
        Kind::Amount(rules) => check_amount(name, value, rules)?,
    };
    Ok(if field.url_encode {
        percent_encode(&checked)
    } else {
        checked
    })
}

/// What a profile produces for one set of attributes.
#[derive(Debug, PartialEq, Eq)]
pub struct Rendered {
    pub payload: String,
    pub label: Option<String>,
}

impl Profile {
    pub fn render_payload(&self, attrs: &[ResolvedAttr]) -> Result<Rendered, String> {
        let mut variables: BTreeMap<&str, &str> = BTreeMap::new();
        let mut label = self.label.clone();
        for dim in &self.dimensions {
            let chosen = match attr(attrs, &dim.name).filter(|v| !v.is_empty()) {
                None => &dim.options[dim.default],
                Some(want) => dim.options.iter().find(|o| o.name == want).ok_or_else(|| {
                    format!(
                        "{} must be one of {}, got \"{want}\"",
                        dim.name,
                        dim.options
                            .iter()
                            .map(|o| o.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })?,
            };
            for (k, v) in &chosen.vars {
                variables.insert(k, v);
            }
            if let Some(l) = &chosen.label {
                label = l.clone();
            }
        }
        let mut values: BTreeMap<&str, String> = BTreeMap::new();
        for (name, field) in &self.fields {
            let raw = attr(attrs, name).ok_or_else(|| format!("{name} is required"))?;
            values.insert(name, check_field(name, field, raw)?);
        }
        let mut payload = String::new();
        for piece in &self.payload {
            match piece {
                Piece::Literal(text) => payload.push_str(text),
                Piece::Reference(name) => match values.get(name.as_str()) {
                    Some(v) => payload.push_str(v),
                    None => {
                        payload.push_str(variables.get(name.as_str()).copied().unwrap_or_default())
                    }
                },
            }
        }
        Ok(Rendered { payload, label })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn error(profile: &str, message: &str) -> Markup {
    html! {
        div class=(invalid_component()) { "FiscalQR (" (profile) "): " (message) }
    }
}

fn render_with(profile: &Profile, attrs: &[ResolvedAttr]) -> Markup {
    let rendered = match profile.render_payload(attrs) {
        Ok(r) => r,
        Err(message) => return error(&profile.id, &message),
    };
    let size = attr(attrs, "size").unwrap_or(&profile.size);
    // Level M is the default and needs no prefix; see `assets::split_qrcode_src`.
    let src = if profile.ecc == 'M' {
        format!("pdfcn-qrcode:{}", hex(rendered.payload.as_bytes()))
    } else {
        format!(
            "pdfcn-qrcode:{}.{}",
            profile.ecc,
            hex(rendered.payload.as_bytes())
        )
    };
    html! {
        div class="inline-block text-center" {
            @if let Some(label) = &rendered.label {
                p class="text-xs font-semibold" { (label) }
            }
            img class="qrcode inline-block" src=(src) style={ "width:" (size) ";height:" (size) ";" } alt="QR code";
        }
    }
}

pub fn fiscal_qr(attrs: &[ResolvedAttr]) -> Markup {
    let Some(id) = attr(attrs, "profile").filter(|p| !p.is_empty()) else {
        return error(
            "?",
            "a \"profile\" is required (the id of a registered profile)",
        );
    };
    let profile = registry().read().ok().and_then(|r| r.get(id).cloned());
    match profile {
        Some(profile) => render_with(&profile, attrs),
        None => error(
            id,
            &format!(
                "no profile with that id is registered (registered: {}); register one with --fiscal-profile",
                fiscal_profile_ids().join(", ")
            ),
        ),
    }
}

/// `%VerifactuQR`: `%FiscalQR(profile="es-verifactu" ...)` under its own name.
pub fn verifactu_qr(attrs: &[ResolvedAttr]) -> Markup {
    let mut with_profile: Vec<ResolvedAttr> = attrs
        .iter()
        .filter(|a| a.name != "profile")
        .cloned()
        .collect();
    with_profile.push(ResolvedAttr {
        name: "profile".into(),
        value: "es-verifactu".into(),
    });
    fiscal_qr(&with_profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(name: &str, value: &str) -> ResolvedAttr {
        ResolvedAttr {
            name: name.into(),
            value: value.into(),
        }
    }

    fn spain() -> Profile {
        Profile::parse(BUILTIN[0]).expect("the built-in profile parses")
    }

    fn url(
        numserie: &str,
        fecha: &str,
        importe: &str,
        extra: &[(&str, &str)],
    ) -> Result<String, String> {
        let mut attrs = vec![
            a("nif", "B43752210"),
            a("numserie", numserie),
            a("fecha", fecha),
            a("importe", importe),
        ];
        // Pre-production unless a test says otherwise (the first match wins).
        if !extra.iter().any(|(k, _)| *k == "env") {
            attrs.push(a("env", "test"));
        }
        attrs.extend(extra.iter().map(|(k, v)| a(k, v)));
        spain().render_payload(&attrs).map(|r| r.payload)
    }

    #[test]
    fn every_builtin_profile_parses() {
        for json in BUILTIN {
            Profile::parse(json).expect("built-in profile");
        }
        assert!(fiscal_profile_ids().contains(&"es-verifactu".to_string()));
    }

    #[test]
    fn spain_builds_the_url_the_agency_documents() {
        assert_eq!(
            url("E-12345", "23-04-2025", "123.45", &[]).unwrap(),
            "https://prewww2.aeat.es/wlpl/TIKE-CONT/ValidarQR?nif=B43752210&numserie=E-12345&fecha=23-04-2025&importe=123.45"
        );
    }

    #[test]
    fn spain_production_and_no_verifactu_use_their_own_endpoints() {
        let production = url("1", "01-01-2026", "1", &[("env", "production")]).unwrap();
        assert!(production
            .starts_with("https://www2.agenciatributaria.gob.es/wlpl/TIKE-CONT/ValidarQR?"));
        let other = url("1", "01-01-2026", "1", &[("mode", "no-verifactu")]).unwrap();
        assert!(other.contains("/ValidarQRNoVerifactu?"), "{other}");
    }

    #[test]
    fn spain_defaults_to_production() {
        let attrs = [
            a("nif", "B43752210"),
            a("numserie", "1"),
            a("fecha", "01-01-2026"),
            a("importe", "1"),
        ];
        let out = spain().render_payload(&attrs).unwrap();
        assert!(
            out.payload
                .starts_with("https://www2.agenciatributaria.gob.es/"),
            "{out:?}"
        );
    }

    #[test]
    fn spain_percent_encodes_the_series() {
        assert!(url("1/25A-321", "23-04-2025", "1", &[])
            .unwrap()
            .contains("numserie=1%2F25A-321&"));
        assert!(url("A 1&b=2", "23-04-2025", "1", &[])
            .unwrap()
            .contains("numserie=A%201%26b%3D2&"));
    }

    #[test]
    fn spain_accepts_iso_dates_and_checks_them() {
        assert!(url("1", "2025-04-23", "1", &[])
            .unwrap()
            .contains("fecha=23-04-2025"));
        assert!(url("1", "2024-02-29", "1", &[]).is_ok());
        assert!(url("1", "2025-02-29", "1", &[]).is_err());
        assert!(url("1", "31-04-2025", "1", &[]).is_err());
        assert!(url("1", "23/04/2025", "1", &[]).is_err());
        assert!(url("1", "3-4-2025", "1", &[])
            .unwrap()
            .contains("fecha=03-04-2025"));
    }

    #[test]
    fn spain_amounts_are_strict() {
        assert!(url("1", "01-01-2026", "194.40", &[])
            .unwrap()
            .ends_with("importe=194.40"));
        assert!(url("1", "01-01-2026", "194,40", &[])
            .unwrap()
            .ends_with("importe=194.40"));
        assert!(url("1", "01-01-2026", "-50", &[])
            .unwrap()
            .ends_with("importe=-50"));
        for bad in [
            "$194.40", "1.194,40", "1,194.40", "12.345", "", "1.", ".5", "1e3",
        ] {
            assert!(
                url("1", "01-01-2026", bad, &[]).is_err(),
                "{bad} should be refused"
            );
        }
    }

    #[test]
    fn spain_checks_the_nif_and_the_series() {
        let build = |nif: &str, serie: &str| {
            spain().render_payload(&[
                a("nif", nif),
                a("numserie", serie),
                a("fecha", "01-01-2026"),
                a("importe", "1"),
            ])
        };
        assert!(build("b43752210", "1")
            .unwrap()
            .payload
            .contains("nif=B43752210"));
        assert!(build("B4375221", "1").is_err());
        assert!(build("B-3752210", "1").is_err());
        assert!(build("B43752210", "").is_err());
        assert!(build("B43752210", &"A".repeat(61)).is_err());
        assert!(build("B43752210", &"A".repeat(60)).is_ok());
        assert!(build("B43752210", "Nº1").is_err());
    }

    #[test]
    fn a_missing_field_and_an_unknown_variant_option_are_named() {
        let err = spain()
            .render_payload(&[
                a("nif", "B43752210"),
                a("fecha", "01-01-2026"),
                a("importe", "1"),
            ])
            .unwrap_err();
        assert!(err.contains("numserie is required"), "{err}");
        let err = url("1", "01-01-2026", "1", &[("env", "staging")]).unwrap_err();
        assert!(err.contains("production, test"), "{err}");
    }

    // ---- the engine, on a profile that is not Spain's --------------------------

    const DEMO: &str = r#"{
        "id": "xx-demo",
        "label": "SCAN",
        "ecc": "H",
        "fields": {
            "tax_id": { "type": "text", "charset": "digits", "length": 6 },
            "kind":   { "type": "text", "allowed": ["FT", "NC"] },
            "date":   { "type": "date", "input": ["yyyy-mm-dd", "dd/mm/yyyy"], "output": "yyyymmdd" },
            "total":  { "type": "amount", "max_decimals": 2, "fixed_decimals": 2, "separator": "," }
        },
        "payload": "A:{tax_id}*B:{kind}*C:{date}*D:{total}*{{ok}}"
    }"#;

    fn demo_attrs() -> Vec<ResolvedAttr> {
        vec![
            a("tax_id", "123456"),
            a("kind", "FT"),
            a("date", "2026-03-04"),
            a("total", "12.5"),
        ]
    }

    #[test]
    fn a_profile_can_describe_a_completely_different_payload() {
        let profile = Profile::parse(DEMO).unwrap();
        let out = profile.render_payload(&demo_attrs()).unwrap();
        assert_eq!(out.payload, "A:123456*B:FT*C:20260304*D:12,50*{ok}");
        assert_eq!(out.label.as_deref(), Some("SCAN"));
        let mut other = demo_attrs();
        other[2] = a("date", "04/03/2026");
        assert_eq!(profile.render_payload(&other).unwrap().payload, out.payload);
        other[1] = a("kind", "XX");
        assert!(profile
            .render_payload(&other)
            .unwrap_err()
            .contains("FT, NC"));
    }

    #[test]
    fn the_error_correction_level_reaches_the_placeholder() {
        let profile = Profile::parse(DEMO).unwrap();
        let out = render_with(&profile, &demo_attrs()).into_string();
        assert!(out.contains("src=\"pdfcn-qrcode:H."), "{out}");
        let spain = render_with(
            &spain(),
            &[
                a("nif", "B43752210"),
                a("numserie", "1"),
                a("fecha", "01-01-2026"),
                a("importe", "1"),
            ],
        )
        .into_string();
        assert!(
            spain.contains("src=\"pdfcn-qrcode:") && !spain.contains("pdfcn-qrcode:M."),
            "{spain}"
        );
    }

    #[test]
    fn a_variant_can_replace_or_drop_the_label() {
        let profile = Profile::parse(
            r#"{ "id": "xx-label", "label": "A", "fields": { "n": { "type": "text" } },
                 "payload": "{p}{n}",
                 "variants": { "m": { "default": "x", "options": {
                     "x": { "vars": { "p": "1" } }, "y": { "vars": { "p": "2" }, "label": null },
                     "z": { "vars": { "p": "3" }, "label": "Z" } } } } }"#,
        )
        .unwrap();
        let get = |m: &str| profile.render_payload(&[a("n", "q"), a("m", m)]).unwrap();
        assert_eq!(get("x").label.as_deref(), Some("A"));
        assert_eq!(get("y").label, None);
        assert_eq!(get("z").label.as_deref(), Some("Z"));
        assert_eq!(get("z").payload, "3q");
    }

    #[test]
    fn registration_catches_mistakes_up_front() {
        let bad = |json: &str| Profile::parse(json).unwrap_err();
        // A placeholder nothing defines.
        assert!(
            bad(r#"{"id":"a","fields":{"n":{"type":"text"}},"payload":"{n}{oops}"}"#)
                .contains("oops")
        );
        // A field the payload never uses.
        assert!(bad(
            r#"{"id":"a","fields":{"n":{"type":"text"},"m":{"type":"text"}},"payload":"{n}"}"#
        )
        .contains("never used"));
        // A variable that only some options define.
        assert!(bad(
            r#"{"id":"a","fields":{"n":{"type":"text"}},"payload":"{n}{v}",
            "variants":{"e":{"default":"x","options":{"x":{"vars":{"v":"1"}},"y":{}}}}}"#
        )
        .contains("{v}"));
        // A variant must name its default explicitly, and it must exist.
        assert!(bad(
            r#"{"id":"a","fields":{"n":{"type":"text"}},"payload":"{n}{v}",
            "variants":{"e":{"options":{"x":{"vars":{"v":"1"}}}}}}"#
        )
        .contains("default"));
        assert!(bad(
            r#"{"id":"a","fields":{"n":{"type":"text"}},"payload":"{n}{v}",
            "variants":{"e":{"default":"z","options":{"x":{"vars":{"v":"1"}}}}}}"#
        )
        .contains("not one of"));
        // A misspelt rule must not silently switch the rule off.
        assert!(
            bad(r#"{"id":"a","fields":{"n":{"type":"text","lenght":9}},"payload":"{n}"}"#)
                .contains("lenght")
        );
        // Bad ids, types and formats.
        assert!(bad(r#"{"id":"Bad Id","fields":{},"payload":"x"}"#).contains("id"));
        assert!(
            bad(r#"{"id":"a","fields":{"n":{"type":"blob"}},"payload":"{n}"}"#).contains("type")
        );
        assert!(bad(
            r#"{"id":"a","fields":{"d":{"type":"date","output":"dd-mm"}},"payload":"{d}"}"#
        )
        .contains("yyyy"));
        assert!(bad(r#"{"id":"a","ecc":"Z","fields":{},"payload":"x"}"#).contains("ecc"));
        assert!(bad("not json").contains("JSON"));
        assert!(
            bad(r#"{"id":"a","fields":{"n":{"type":"text"}},"payload":"{n"}"#).contains("unclosed")
        );
        // A field cannot shadow an attribute the component itself reads.
        assert!(
            bad(r#"{"id":"a","fields":{"size":{"type":"text"}},"payload":"{size}"}"#)
                .contains("reserved")
        );
    }

    #[test]
    fn registered_profiles_are_used_by_the_component_and_can_replace_others() {
        register_fiscal_profile(&DEMO.replace("xx-demo", "xx-registered")).unwrap();
        let mut attrs = demo_attrs();
        attrs.push(a("profile", "xx-registered"));
        let out = fiscal_qr(&attrs).into_string();
        assert!(out.contains("src=\"pdfcn-qrcode:H."), "{out}");
        assert!(out.contains("SCAN"), "{out}");

        // Registering the same id again replaces it.
        register_fiscal_profile(
            &DEMO
                .replace("xx-demo", "xx-registered")
                .replace("SCAN", "AGAIN"),
        )
        .unwrap();
        assert!(fiscal_qr(&attrs).into_string().contains("AGAIN"));
    }

    #[test]
    fn the_component_reports_a_missing_or_unknown_profile() {
        let out = fiscal_qr(&[]).into_string();
        assert!(
            out.contains("pdfcn-invalid-component") && out.contains("profile"),
            "{out}"
        );
        let out = fiscal_qr(&[a("profile", "xx-nowhere")]).into_string();
        assert!(
            out.contains("es-verifactu"),
            "lists what is registered: {out}"
        );
        assert!(!out.contains("pdfcn-qrcode:"));
    }

    fn verifactu_attrs() -> Vec<ResolvedAttr> {
        vec![
            a("nif", "B43752210"),
            a("numserie", "E-12345"),
            a("fecha", "2025-04-23"),
            a("importe", "123.45"),
        ]
    }

    #[test]
    fn the_verifactu_shorthand_prints_the_label_and_a_qr() {
        let out = verifactu_qr(&verifactu_attrs()).into_string();
        assert!(out.contains("VERI*FACTU"), "{out}");
        assert!(out.contains("src=\"pdfcn-qrcode:"), "{out}");
    }

    #[test]
    fn the_verifactu_shorthand_drops_the_label_in_no_verifactu_mode() {
        let mut attrs = verifactu_attrs();
        attrs.push(a("mode", "no-verifactu"));
        assert!(!verifactu_qr(&attrs).into_string().contains("VERI*FACTU"));
    }

    #[test]
    fn bad_data_is_a_visible_error_never_a_wrong_qr() {
        let mut attrs = verifactu_attrs();
        attrs[3] = a("importe", "$123.45");
        let out = verifactu_qr(&attrs).into_string();
        assert!(
            out.contains("pdfcn-invalid-component") && out.contains("importe"),
            "{out}"
        );
        assert!(!out.contains("pdfcn-qrcode:"), "{out}");
    }
}
