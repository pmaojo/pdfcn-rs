//! PAdES signing: the signed PDF must keep the original bytes, describe the
//! signed ranges correctly, and carry a CMS whose digest and signature check
//! out against the signer's certificate -- for every key format we accept.
#![cfg(feature = "sign")]

use std::path::PathBuf;

use cms::content_info::ContentInfo;
use cms::signed_data::SignedData;
use der::{Decode, Encode};
use pdfcn_core::{
    render_html, render_pdf, sign_pdf, NoPartials, RenderOptions, SignOptions, SigningIdentity,
};
use sha2::{Digest, Sha256};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sign")
        .join(name)
}

fn text(name: &str) -> String {
    std::fs::read_to_string(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn bytes(name: &str) -> Vec<u8> {
    std::fs::read(fixture(name)).unwrap_or_else(|e| panic!("{name}: {e}"))
}

fn document() -> Vec<u8> {
    let template = "%DocumentLayout(size=\"a4\")\n  %h1 Invoice 2026-001\n  %p Total 100.00 EUR\n";
    let html = render_html(template, &serde_json::json!({}), &NoPartials).expect("html");
    render_pdf(&html, &RenderOptions::default()).expect("pdf")
}

fn find(h: &[u8], n: &[u8]) -> Option<usize> {
    h.windows(n.len()).position(|w| w == n)
}

/// Checks everything a verifier checks, using only public crypto.
fn assert_valid_signature(original: &[u8], signed: &[u8], identity_cert_pem: &str) {
    assert!(
        signed.starts_with(original),
        "the original bytes must be untouched"
    );

    let at = original.len() + find(&signed[original.len()..], b"/ByteRange").expect("ByteRange");
    let open = at + find(&signed[at..], b"[").unwrap() + 1;
    let close = open + find(&signed[open..], b"]").unwrap();
    let nums: Vec<usize> = std::str::from_utf8(&signed[open..close])
        .unwrap()
        .split_whitespace()
        .map(|n| n.parse().unwrap())
        .collect();
    assert_eq!(nums.len(), 4, "{nums:?}");
    assert_eq!(nums[0], 0);
    assert_eq!(
        nums[2] + nums[3],
        signed.len(),
        "ranges must reach the end of the file"
    );
    assert_eq!(signed[nums[1]], b'<');
    assert_eq!(signed[nums[2] - 1], b'>');

    let hex = std::str::from_utf8(&signed[nums[1] + 1..nums[2] - 1]).unwrap();
    let raw: Vec<u8> = (0..hex.len() / 2)
        .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
        .collect();
    // The CMS is DER, zero-padded to the reserved size.
    let mut reader = der::SliceReader::new(&raw).expect("reader");
    let info = ContentInfo::decode(&mut reader).expect("CMS ContentInfo");
    assert!(
        raw[usize::try_from(der::Reader::position(&reader)).unwrap()..]
            .iter()
            .all(|b| *b == 0),
        "only zero padding may follow"
    );
    let data: SignedData = info.content.decode_as().expect("SignedData");
    assert!(
        data.encap_content_info.econtent.is_none(),
        "must be detached"
    );
    assert!(data.certificates.is_some());

    let signer = data.signer_infos.0.iter().next().expect("one signer");
    let attrs = signer.signed_attrs.as_ref().expect("signed attributes");

    let mut covered = signed[..nums[1]].to_vec();
    covered.extend_from_slice(&signed[nums[2]..]);
    let expected = Sha256::digest(&covered);
    let message_digest = attrs
        .iter()
        .find(|a| a.oid.to_string() == "1.2.840.113549.1.9.4")
        .expect("messageDigest");
    let got = message_digest.values.iter().next().unwrap().value();
    assert_eq!(
        got,
        expected.as_slice(),
        "messageDigest must hash the signed ranges"
    );
    assert!(
        attrs
            .iter()
            .any(|a| a.oid.to_string() == "1.2.840.113549.1.9.16.2.47"),
        "PAdES needs signing-certificate-v2"
    );

    // The signature is over the DER of the attributes as a SET.
    let signed_attrs = attrs.to_der().expect("attrs der");
    let cert = x509_cert::Certificate::load_pem_chain(identity_cert_pem.as_bytes())
        .unwrap()
        .remove(0);
    let spki = cert.tbs_certificate.subject_public_key_info;
    let sig = signer.signature.as_bytes();
    match spki.algorithm.oid.to_string().as_str() {
        "1.2.840.113549.1.1.1" => {
            use rsa::pkcs1v15::{Signature, VerifyingKey};
            use rsa::pkcs8::DecodePublicKey;
            use rsa::signature::Verifier;
            let key = rsa::RsaPublicKey::from_public_key_der(&spki.to_der().unwrap()).unwrap();
            VerifyingKey::<Sha256>::new(key)
                .verify(&signed_attrs, &Signature::try_from(sig).unwrap())
                .expect("RSA signature must verify");
        }
        "1.2.840.10045.2.1" => {
            use p256::ecdsa::signature::Verifier;
            use p256::ecdsa::{DerSignature, VerifyingKey};
            let key = VerifyingKey::from_sec1_bytes(spki.subject_public_key.raw_bytes()).unwrap();
            key.verify(&signed_attrs, &DerSignature::try_from(sig).unwrap())
                .expect("ECDSA signature must verify");
        }
        other => panic!("unexpected key algorithm {other}"),
    }
}

fn check(identity: SigningIdentity, cert: &str) {
    let original = document();
    let signed = sign_pdf(&original, &identity, &SignOptions::default()).expect("sign");
    assert_valid_signature(&original, &signed, &text(cert));
    // Still a PDF a parser accepts, now with an AcroForm signature field.
    let doc = lopdf::Document::load_mem(&signed).expect("signed PDF re-parses");
    assert!(doc.catalog().unwrap().get(b"AcroForm").is_ok());
    if let Some(dir) = std::env::var_os("PDFCN_SIGN_OUT") {
        let name = format!("{}.pdf", cert.trim_end_matches(".cert.pem"));
        std::fs::write(PathBuf::from(dir).join(name), &signed).unwrap();
    }
}

#[test]
fn rsa_pem() {
    check(
        SigningIdentity::from_pem(&text("rsa.cert.pem"), &text("rsa.key.pem"), None).unwrap(),
        "rsa.cert.pem",
    );
}

#[test]
fn ecdsa_pem() {
    check(
        SigningIdentity::from_pem(&text("ec.cert.pem"), &text("ec.key.pem"), None).unwrap(),
        "ec.cert.pem",
    );
}

#[test]
fn legacy_key_formats() {
    check(
        SigningIdentity::from_pem(&text("rsa.cert.pem"), &text("rsa.pkcs1.key.pem"), None).unwrap(),
        "rsa.cert.pem",
    );
    check(
        SigningIdentity::from_pem(&text("ec.cert.pem"), &text("ec.sec1.key.pem"), None).unwrap(),
        "ec.cert.pem",
    );
}

#[test]
fn encrypted_key_needs_the_right_password() {
    let (cert, key) = (text("rsa.cert.pem"), text("rsa.enc.key.pem"));
    assert!(SigningIdentity::from_pem(&cert, &key, None).is_err());
    assert!(SigningIdentity::from_pem(&cert, &key, Some("wrong")).is_err());
    check(
        SigningIdentity::from_pem(&cert, &key, Some("secret")).unwrap(),
        "rsa.cert.pem",
    );
}

#[test]
fn pkcs12_modern_and_legacy() {
    check(
        SigningIdentity::from_pkcs12(&bytes("rsa.p12"), "secret").unwrap(),
        "rsa.cert.pem",
    );
    check(
        SigningIdentity::from_pkcs12(&bytes("ec-legacy.p12"), "secret").unwrap(),
        "ec.cert.pem",
    );
    assert!(SigningIdentity::from_pkcs12(&bytes("rsa.p12"), "nope").is_err());
}

#[test]
fn tampering_after_signing_breaks_the_digest() {
    let original = document();
    let identity =
        SigningIdentity::from_pem(&text("rsa.cert.pem"), &text("rsa.key.pem"), None).unwrap();
    let mut signed = sign_pdf(&original, &identity, &SignOptions::default()).unwrap();
    // Any byte inside the signed range will do.
    signed[100] ^= 0x01;
    let result = std::panic::catch_unwind(|| {
        assert_valid_signature(&original, &signed, &text("rsa.cert.pem"))
    });
    assert!(
        result.is_err(),
        "a modified document must fail verification"
    );
}

#[test]
fn a_second_signature_is_added_as_a_new_revision() {
    let original = document();
    let a = SigningIdentity::from_pem(&text("rsa.cert.pem"), &text("rsa.key.pem"), None).unwrap();
    let b = SigningIdentity::from_pem(&text("ec.cert.pem"), &text("ec.key.pem"), None).unwrap();
    let once = sign_pdf(&original, &a, &SignOptions::default()).unwrap();
    let twice = sign_pdf(
        &once,
        &b,
        &SignOptions {
            reason: Some("approved".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(
        twice.starts_with(&once),
        "the first signature's bytes must stay intact"
    );
    assert_valid_signature(&once, &twice, &text("ec.cert.pem"));
    let doc = lopdf::Document::load_mem(&twice).unwrap();
    let form = doc.catalog().unwrap().get(b"AcroForm").unwrap();
    let fields = match form {
        lopdf::Object::Dictionary(d) => d.get(b"Fields").unwrap().as_array().unwrap().len(),
        lopdf::Object::Reference(id) => doc
            .get_object(*id)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Fields")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        _ => 0,
    };
    assert_eq!(fields, 2);
}
