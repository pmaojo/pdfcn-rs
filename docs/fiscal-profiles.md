# Fiscal QR profiles

Many tax authorities ask for a QR code on every invoice. They all ask for a
different one, and the rules change. pdfcn does not hard-code any country:
`%FiscalQR` is driven by a **profile**, a small JSON file that says what the
authority's specification says. Spain's VERI*FACTU ships as one such profile
(`es-verifactu`); for any other country you write one, using the specification
you were given, and pass it with `--fiscal-profile`.

```sh
pdfcn build invoice.haml -d invoice.json --fiscal-profile my-country.json
```

```haml
%FiscalQR(profile="my-country" tax_id="{{ issuer.tax_id }}" number="{{ invoice.number }}" date="{{ invoice.date }}" total="{{ invoice.total }}")
```

From Rust, register it once at start-up with
`pdfcn_core::register_fiscal_profile(json)`.

A profile describes the **printed code**. It does not create the invoice record,
chain hashes, sign records or talk to the authority; see "What a profile cannot
do" below.

## Format

```json
{
  "id": "xx-demo",
  "description": "free text, ignored",
  "label": "SCAN TO VERIFY",
  "size": "140px",
  "ecc": "H",
  "fields": { "...": "..." },
  "payload": "{base}?t={tax_id}*d={date}",
  "variants": { "...": "..." }
}
```

| Key | Meaning |
|---|---|
| `id` | Lowercase letters, digits, `-`, `_`. What `profile="..."` refers to. A profile with an existing id replaces it, built-ins included, so a project can pin its own reading of a specification. |
| `label` | Optional text printed above the symbol (VERI*FACTU requires one). |
| `size` | Default side length (`150px` is about 40 mm at the engine's 96 px/in); the component's `size=` overrides it. |
| `ecc` | Error-correction level `L`, `M` (default), `Q` or `H`. |
| `fields` | The values the invoice supplies, each with its validation. Every field is **required** and must appear in the payload. |
| `payload` | The text the QR encodes. `{name}` inserts a field or variable; `{{` and `}}` are literal braces. It need not be a URL. |
| `variants` | Switches such as environment or mode, chosen with an attribute of the same name. |

Unknown keys are errors: a misspelt rule would otherwise silently switch the
rule off. A profile is checked when it is loaded (unknown placeholders,
unused fields, a variable that only some options define, a missing default), so
mistakes show up at start-up and not on the first printed invoice.

### Fields

Every field has `"type"` and optionally `"encode": "url"` to percent-encode
the value in the payload (RFC 3986; use it for free text inside a URL).

**`text`**: `charset` (`any`, `alnum`, `digits`, `printable_ascii`), `length`
or `min`/`max` (in characters), `uppercase`, `trim` (default true), `allowed` (a
list of accepted values).

**`date`**: `input` (a list of accepted formats; defaults to `output`) and
`output`, written with `dd`, `mm`, `yyyy` and separators, e.g. `yyyy-mm-dd`,
`dd/mm/yyyy`, `yyyymmdd`. The date must exist (29 February only in leap years).

**`amount`**: `max_integer_digits`, `max_decimals`, `fixed_decimals` (pads to
exactly that many), `allow_negative`, `decimal_comma` (also accept `12,5` as
input), `separator` (`.` or `,` in the payload). Currency symbols and
thousands separators are refused rather than guessed.

### Variants

```json
"variants": {
  "env": {
    "default": "production",
    "options": {
      "production": { "vars": { "base": "https://verify.example.org/check" } },
      "test":       { "vars": { "base": "https://sandbox.verify.example.org/check" } }
    }
  }
}
```

The attribute `env="test"` selects an option; without it, `default` applies. An
option's `vars` fill `{placeholders}` in the payload, and its `label` replaces
the profile's (`null` removes it). Variants of different names combine.
Spain's profile uses two: `env` (production/test) and `mode`
(verifactu/no-verifactu).

## Writing one for your country

1. Take the authority's QR specification (what the QR contains, in which
   order, how each value is formatted, the error-correction level, the printed
   size and any label).
2. Write a field for each value and the `payload` template that joins them.
3. Print an invoice and **scan the result** with a phone or a decoder. The
   authority's own verification service, if it has a test environment, is the
   real check.

`examples/fiscal-profiles/xx-demo.json` is a complete profile (a made-up one,
not any real country's rules) and `examples/fiscal-demo.haml` uses it. Profiles
for real countries are only as good as the specification they were written
from; this repository ships only Spain's, and flags that it was written from
secondary sources.

## What a profile cannot do

- **Optional fields.** Every field is required. A specification whose payload
  skips absent values needs one profile per case, selected with a variant.
- **Computed values**, such as a check digit, a hash or a signature, over other
  fields. Compute those in your billing system and pass the result as a field.
- **Anything beyond the printed code**: the invoice record, its hash chain,
  signing it and submitting it. Those need persistent state, an event log and
  often a qualified certificate, which belong to the billing system, not to a
  renderer. Where a regulation treats the component that prints invoices or
  generates the QR as part of the certified invoicing system (Spain's does), the
  certification obligations of that system apply to how you use pdfcn.
