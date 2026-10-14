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

// ---------------------------------------------------------------------------
// Timestamps (PAdES-B-T). The authority is simulated: a real TSA is a network
// service, and pdfcn-core takes it as a function precisely so it can be.
// ---------------------------------------------------------------------------

mod timestamp {
    use super::*;
    use cms::builder::{SignedDataBuilder, SignerInfoBuilder};
    use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
    use cms::signed_data::{EncapsulatedContentInfo, SignerIdentifier};
    use der::asn1::{GeneralizedTime, Int, OctetString};
    use der::Any;
    use pdfcn_core::sign_pdf_timestamped;
    use spki::AlgorithmIdentifierOwned;
    use x509_tsp::{TimeStampReq, TimeStampResp, TspVersion, TstInfo};

    const OID_TST_INFO: der::oid::ObjectIdentifier =
        der::oid::ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.1.4");

    /// What a well-behaved TSA does with a request. `tamper` lets a test make
    /// it misbehave.
    fn respond(request: &[u8], tamper: &dyn Fn(&mut TstInfo)) -> Result<Vec<u8>, String> {
        let request = TimeStampReq::from_der(request).map_err(|e| e.to_string())?;
        assert!(
            request.cert_req,
            "the signer should ask for the TSA certificate"
        );

        let key = p256::ecdsa::SigningKey::from_pkcs8_pem(&text("tsa.key.pem"))
            .map_err(|e| e.to_string())?;
        let cert = x509_cert::Certificate::load_pem_chain(text("tsa.cert.pem").as_bytes())
            .map_err(|e| e.to_string())?
            .remove(0);

        let mut tst = TstInfo {
            version: TspVersion::V1,
            policy: der::oid::ObjectIdentifier::new_unwrap("1.2.3.4.1"),
            message_imprint: request.message_imprint.clone(),
            serial_number: Int::new(&[1]).unwrap(),
            gen_time: GeneralizedTime::from_unix_duration(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap(),
            )
            .unwrap(),
            accuracy: None,
            ordering: false,
            nonce: request.nonce.clone(),
            tsa: None,
            extensions: None,
        };
        tamper(&mut tst);

        let content = EncapsulatedContentInfo {
            econtent_type: OID_TST_INFO,
            econtent: Some(
                Any::encode_from(&OctetString::new(tst.to_der().unwrap()).unwrap()).unwrap(),
            ),
        };
        let sha256 = AlgorithmIdentifierOwned {
            oid: const_oid::ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1"),
            parameters: None,
        };
        let sid = SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
            issuer: cert.tbs_certificate.issuer.clone(),
            serial_number: cert.tbs_certificate.serial_number.clone(),
        });
        let signer = SignerInfoBuilder::new(&key, sid, sha256.clone(), &content, None)
            .map_err(|e| e.to_string())?;
        let mut builder = SignedDataBuilder::new(&content);
        builder
            .add_digest_algorithm(sha256)
            .map_err(|e| e.to_string())?;
        builder
            .add_certificate(CertificateChoices::Certificate(cert))
            .map_err(|e| e.to_string())?;
        builder
            .add_signer_info::<_, p256::ecdsa::DerSignature>(signer)
            .map_err(|e| e.to_string())?;
        let token = builder.build().map_err(|e| e.to_string())?;

        let response = TimeStampResp {
            status: cmpv2::status::PkiStatusInfo {
                status: cmpv2::status::PkiStatus::Accepted,
                status_string: None,
                fail_info: None,
            },
            time_stamp_token: Some(token),
        };
        response.to_der().map_err(|e| e.to_string())
    }

    fn identity() -> SigningIdentity {
        SigningIdentity::from_pem(&text("rsa.cert.pem"), &text("rsa.key.pem"), None).unwrap()
    }

    use p256::pkcs8::DecodePrivateKey;

    #[test]
    fn a_timestamp_token_is_attached_as_an_unsigned_attribute() {
        let original = document();
        let signed =
            sign_pdf_timestamped(&original, &identity(), &SignOptions::default(), &|req| {
                respond(req, &|_| {})
            })
            .expect("sign with timestamp");
        // The signature itself is still valid: the token sits outside what it covers.
        assert_valid_signature(&original, &signed, &text("rsa.cert.pem"));

        // And the token is there, for this very signature.
        // `/Contents` also names the page's content stream: start at the signature.
        let range = original.len() + find(&signed[original.len()..], b"/ByteRange").unwrap();
        let at = range + find(&signed[range..], b"/Contents").unwrap();
        let open = at + find(&signed[at..], b"<").unwrap();
        let close = open + find(&signed[open..], b">").unwrap();
        let hex = std::str::from_utf8(&signed[open + 1..close]).unwrap();
        let raw: Vec<u8> = (0..hex.len() / 2)
            .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).unwrap())
            .collect();
        let mut reader = der::SliceReader::new(&raw).unwrap();
        let info = ContentInfo::decode(&mut reader).unwrap();
        let data: SignedData = info.content.decode_as().unwrap();
        let signer = data.signer_infos.0.iter().next().unwrap();
        let unsigned = signer.unsigned_attrs.as_ref().expect("unsigned attributes");
        let attribute = unsigned.iter().next().unwrap();
        assert_eq!(attribute.oid.to_string(), "1.2.840.113549.1.9.16.2.14");

        let token: ContentInfo = attribute.values.iter().next().unwrap().decode_as().unwrap();
        let token_data: SignedData = token.content.decode_as().unwrap();
        let octets: OctetString = token_data
            .encap_content_info
            .econtent
            .unwrap()
            .decode_as()
            .unwrap();
        let tst = TstInfo::from_der(octets.as_bytes()).unwrap();
        assert_eq!(
            tst.message_imprint.hashed_message.as_bytes(),
            Sha256::digest(signer.signature.as_bytes()).as_slice(),
            "the timestamp must be over the signature value"
        );
    }

    #[test]
    fn a_response_for_another_signature_is_rejected() {
        let result =
            sign_pdf_timestamped(&document(), &identity(), &SignOptions::default(), &|req| {
                respond(req, &|tst| {
                    let mut bytes = tst.message_imprint.hashed_message.as_bytes().to_vec();
                    bytes[0] ^= 0xFF;
                    tst.message_imprint.hashed_message = OctetString::new(bytes).unwrap();
                })
            });
        assert!(result.unwrap_err().to_string().contains("imprint"));
    }

    #[test]
    fn a_response_with_the_wrong_nonce_is_rejected() {
        let result =
            sign_pdf_timestamped(&document(), &identity(), &SignOptions::default(), &|req| {
                respond(req, &|tst| tst.nonce = Some(Int::new(&[42]).unwrap()))
            });
        assert!(result.unwrap_err().to_string().contains("nonce"));
    }

    #[test]
    fn an_unreachable_authority_is_an_error_not_an_unstamped_signature() {
        let result =
            sign_pdf_timestamped(&document(), &identity(), &SignOptions::default(), &|_| {
                Err("connection refused".into())
            });
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("connection refused"));
    }
}
