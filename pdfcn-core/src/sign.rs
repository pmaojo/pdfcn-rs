//! PAdES digital signatures for a finished PDF, behind pdfcn-core's opt-in
//! `sign` cargo feature.
//!
//! The signature is a CMS `SignedData` (detached, SHA-256) with the
//! `signing-certificate-v2` attribute, stored under `/SubFilter
//! /ETSI.CAdES.detached` -- the PAdES baseline-B profile (PAdES-B-B). It is
//! appended as a PDF *incremental update*, so every byte of the document
//! that was rendered (including a Factur-X attachment) stays exactly as it
//! was and is covered by the signature.
//!
//! No timestamp or revocation data is embedded (that would be PAdES-B-T/LT),
//! and the signing time is only the informational `/M` entry.
//!
//! Nothing here panics; bad input comes back as `Err(SignError)`.

use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use cms::builder::{SignedDataBuilder, SignerInfoBuilder};
use cms::cert::{CertificateChoices, IssuerAndSerialNumber};
use cms::content_info::ContentInfo;
use cms::signed_data::{EncapsulatedContentInfo, SignedData, SignerIdentifier};
use const_oid::ObjectIdentifier;
use der::asn1::{OctetString, SetOfVec};
use der::{Any, Decode, Encode, Reader};
use lopdf::{dictionary, Dictionary, IncrementalDocument, Object, ObjectId, StringFormat};
use rsa::pkcs1::DecodeRsaPrivateKey;
use rsa::pkcs8::DecodePrivateKey;
use sha2::{Digest, Sha256};
use spki::AlgorithmIdentifierOwned;
use x509_cert::attr::Attribute;
use x509_cert::Certificate;

/// Slack on top of the measured CMS size: an ECDSA signature's DER length
/// varies by a few bytes from one signing to the next.
const RESERVE_MARGIN: usize = 64;

/// Stands in for each `/ByteRange` offset until the real ones are known. The
/// real numbers are never longer, so they are written over it in place.
const RANGE_PLACEHOLDER: i64 = 9_999_999_999;

const OID_SHA256: ObjectIdentifier = ObjectIdentifier::new_unwrap("2.16.840.1.101.3.4.2.1");
const OID_SIGNING_CERT_V2: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.47");
const OID_DATA: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.113549.1.7.1");

#[derive(Debug)]
pub struct SignError(String);

impl fmt::Display for SignError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SignError {}

fn err<T>(message: impl Into<String>) -> Result<T, SignError> {
    Err(SignError(message.into()))
}

impl From<lopdf::Error> for SignError {
    fn from(e: lopdf::Error) -> Self {
        SignError(format!("PDF structure: {e}"))
    }
}

impl From<std::io::Error> for SignError {
    fn from(e: std::io::Error) -> Self {
        SignError(format!("writing the PDF: {e}"))
    }
}

impl From<der::Error> for SignError {
    fn from(e: der::Error) -> Self {
        SignError(format!("ASN.1: {e}"))
    }
}

enum Key {
    Rsa(Box<rsa::pkcs1v15::SigningKey<Sha256>>),
    P256(p256::ecdsa::SigningKey),
}

/// A signing certificate chain with its private key.
///
/// The first certificate must be the signer's own; any that follow are
/// intermediates, embedded so verifiers can build the path.
pub struct SigningIdentity {
    key: Key,
    chain: Vec<Certificate>,
}

impl SigningIdentity {
    /// From PEM text: a certificate chain (signer first) and a private key
    /// (`PRIVATE KEY`, `ENCRYPTED PRIVATE KEY` with `password`,
    /// `RSA PRIVATE KEY` or `EC PRIVATE KEY`).
    pub fn from_pem(
        chain_pem: &str,
        key_pem: &str,
        password: Option<&str>,
    ) -> Result<Self, SignError> {
        let chain = Certificate::load_pem_chain(chain_pem.as_bytes())
            .map_err(|e| SignError(format!("certificate PEM: {e}")))?;
        if chain.is_empty() {
            return err("no CERTIFICATE block found in the certificate PEM");
        }
        let (label, body) = der::pem::decode_vec(key_pem.as_bytes())
            .map_err(|e| SignError(format!("private key PEM: {e}")))?;
        let key = match label {
            "PRIVATE KEY" => key_from_pkcs8(&body)?,
            "ENCRYPTED PRIVATE KEY" => {
                let Some(password) = password else {
                    return err("the private key is encrypted: a password is required");
                };
                let info = pkcs8::EncryptedPrivateKeyInfo::try_from(body.as_slice())
                    .map_err(|e| SignError(format!("encrypted key: {e}")))?;
                let plain = info
                    .decrypt(password)
                    .map_err(|_| SignError("wrong private key password".into()))?;
                key_from_pkcs8(plain.as_bytes())?
            }
            "RSA PRIVATE KEY" => Key::Rsa(Box::new(rsa::pkcs1v15::SigningKey::new(
                rsa::RsaPrivateKey::from_pkcs1_der(&body)
                    .map_err(|e| SignError(format!("RSA key: {e}")))?,
            ))),
            "EC PRIVATE KEY" => Key::P256(
                p256::SecretKey::from_sec1_der(&body)
                    .map(Into::into)
                    .map_err(|e| SignError(format!("EC key (only P-256 is supported): {e}")))?,
            ),
            other => return err(format!("unsupported private key PEM label `{other}`")),
        };
        Ok(Self { key, chain })
    }

    /// From a PKCS#12 (`.p12` / `.pfx`) file, including the legacy
    /// 3DES/RC2-protected ones older tools export.
    pub fn from_pkcs12(bytes: &[u8], password: &str) -> Result<Self, SignError> {
        let store = p12_keystore::KeyStore::from_pkcs12(bytes, password)
            .map_err(|e| SignError(format!("PKCS#12: {e}")))?;
        let Some((_, entry)) = store.private_key_chain() else {
            return err("the PKCS#12 file holds no private key");
        };
        let chain = entry
            .chain()
            .iter()
            .map(|c| Certificate::from_der(c.as_der()))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| SignError(format!("PKCS#12 certificate: {e}")))?;
        if chain.is_empty() {
            return err("the PKCS#12 key has no certificate chain");
        }
        Ok(Self {
            key: key_from_pkcs8(entry.key())?,
            chain,
        })
    }

    /// The signer certificate's subject, e.g. `CN=Acme SL,O=Acme,C=ES`.
    pub fn subject(&self) -> String {
        self.chain[0].tbs_certificate.subject.to_string()
    }
}

fn key_from_pkcs8(der_bytes: &[u8]) -> Result<Key, SignError> {
    if let Ok(k) = rsa::RsaPrivateKey::from_pkcs8_der(der_bytes) {
        return Ok(Key::Rsa(Box::new(rsa::pkcs1v15::SigningKey::new(k))));
    }
    if let Ok(k) = p256::ecdsa::SigningKey::from_pkcs8_der(der_bytes) {
        return Ok(Key::P256(k));
    }
    err("unsupported private key type (RSA and ECDSA P-256 are supported)")
}

/// What the signature says about itself. All optional.
#[derive(Debug, Clone, Default)]
pub struct SignOptions {
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
    /// Shown as the signer's name; defaults to the certificate's CN.
    pub name: Option<String>,
}

/// How to reach a time-stamping authority: takes a DER `TimeStampReq`
/// (RFC 3161) and returns the DER `TimeStampResp`. pdfcn-core does no network
/// I/O itself, so the transport (HTTP POST with
/// `Content-Type: application/timestamp-query`) belongs to the caller.
pub type TimestampAuthority<'a> = &'a dyn Fn(&[u8]) -> Result<Vec<u8>, String>;

/// Room kept for the timestamp token, which carries the TSA's certificates and
/// whose size is only known after the request has been answered.
const TIMESTAMP_RESERVE: usize = 12 * 1024;

/// Signs `pdf`, returning the PDF with one more revision holding the
/// signature. Needs a classic (non-encrypted) PDF such as pdfcn produces.
pub fn sign_pdf(
    pdf: &[u8],
    identity: &SigningIdentity,
    options: &SignOptions,
) -> Result<Vec<u8>, SignError> {
    sign_impl(pdf, identity, options, None)
}

/// [`sign_pdf`] plus a trusted timestamp from `authority` over the signature
/// (PAdES baseline-T): it proves the document was signed while the
/// certificate was valid, which keeps the signature checkable after the
/// certificate expires.
pub fn sign_pdf_timestamped(
    pdf: &[u8],
    identity: &SigningIdentity,
    options: &SignOptions,
    authority: TimestampAuthority<'_>,
) -> Result<Vec<u8>, SignError> {
    sign_impl(pdf, identity, options, Some(authority))
}

fn sign_impl(
    pdf: &[u8],
    identity: &SigningIdentity,
    options: &SignOptions,
    authority: Option<TimestampAuthority<'_>>,
) -> Result<Vec<u8>, SignError> {
    let previous = lopdf::Document::load_mem(pdf)?;
    let mut doc = IncrementalDocument::create_from(pdf.to_vec(), previous);

    let catalog_id = doc
        .get_prev_documents()
        .trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .map_err(|_| SignError("the PDF has no /Root".into()))?;
    let page_id = doc
        .get_prev_documents()
        .get_pages()
        .get(&1)
        .copied()
        .ok_or_else(|| SignError("the PDF has no pages".into()))?;
    doc.opt_clone_object_to_new_document(catalog_id)?;
    doc.opt_clone_object_to_new_document(page_id)?;

    // The CMS length depends on the certificates and the key, not on the
    // digest, so a dry run with a dummy digest says how much room to leave.
    let reserve = build_cms(identity, &[0; 32], None)?.len()
        + RESERVE_MARGIN
        + if authority.is_some() {
            TIMESTAMP_RESERVE
        } else {
            0
        };

    let signer_name = options
        .name
        .clone()
        .unwrap_or_else(|| common_name(identity));
    let mut signature = dictionary! {
        "Type" => "Sig",
        "Filter" => "Adobe.PPKLite",
        "SubFilter" => "ETSI.CAdES.detached",
        "ByteRange" => vec![
            Object::Integer(0),
            Object::Integer(RANGE_PLACEHOLDER),
            Object::Integer(RANGE_PLACEHOLDER),
            Object::Integer(RANGE_PLACEHOLDER),
        ],
        "Contents" => Object::String(vec![0; reserve], StringFormat::Hexadecimal),
        "M" => Object::string_literal(pdf_date()),
        "Name" => Object::string_literal(signer_name),
    };
    for (key, value) in [
        ("Reason", &options.reason),
        ("Location", &options.location),
        ("ContactInfo", &options.contact_info),
    ] {
        if let Some(v) = value {
            signature.set(key, Object::string_literal(v.as_str()));
        }
    }
    let signature_id = doc.new_document.add_object(signature);

    let field_name = next_field_name(doc.get_prev_documents(), catalog_id);
    let field_id = doc.new_document.add_object(dictionary! {
        "Type" => "Annot",
        "Subtype" => "Widget",
        "FT" => "Sig",
        "T" => Object::string_literal(field_name),
        "V" => signature_id,
        "F" => 132,
        "Rect" => vec![0.into(), 0.into(), 0.into(), 0.into()],
        "P" => page_id,
    });

    attach_widget(&mut doc, page_id, field_id)?;
    attach_acroform(&mut doc, catalog_id, field_id)?;

    let mut out = Vec::with_capacity(pdf.len() + 4 * reserve);
    doc.save_to(&mut out)?;
    fill_signature(out, pdf.len(), identity, reserve, authority)
}

fn common_name(identity: &SigningIdentity) -> String {
    let subject = identity.subject();
    subject
        .split(',')
        .find_map(|part| part.trim().strip_prefix("CN=").map(str::to_string))
        .unwrap_or(subject)
}

/// `Signature1`, `Signature2`, ... -- the first name no existing field uses.
fn next_field_name(prev: &lopdf::Document, catalog_id: ObjectId) -> String {
    let mut taken = 0usize;
    if let Ok(Object::Dictionary(catalog)) = prev.get_object(catalog_id) {
        if let Ok(form) = catalog.get(b"AcroForm").and_then(|o| resolve_dict(prev, o)) {
            if let Ok(fields) = form.get(b"Fields").and_then(|o| resolve_array(prev, o)) {
                taken = fields.len();
            }
        }
    }
    format!("Signature{}", taken + 1)
}

fn resolve_dict<'a>(doc: &'a lopdf::Document, o: &'a Object) -> lopdf::Result<&'a Dictionary> {
    match o {
        Object::Reference(id) => doc.get_object(*id)?.as_dict(),
        other => other.as_dict(),
    }
}

fn resolve_array<'a>(doc: &'a lopdf::Document, o: &'a Object) -> lopdf::Result<&'a Vec<Object>> {
    match o {
        Object::Reference(id) => doc.get_object(*id)?.as_array(),
        other => other.as_array(),
    }
}

/// Adds the widget to the first page's `/Annots`, whether that array is
/// direct, indirect or missing.
fn attach_widget(
    doc: &mut IncrementalDocument,
    page_id: ObjectId,
    field_id: ObjectId,
) -> Result<(), SignError> {
    let existing = doc
        .new_document
        .get_object(page_id)?
        .as_dict()?
        .get(b"Annots")
        .ok()
        .cloned();
    match existing {
        Some(Object::Reference(array_id)) => {
            doc.opt_clone_object_to_new_document(array_id)?;
            doc.new_document
                .get_object_mut(array_id)?
                .as_array_mut()?
                .push(field_id.into());
        }
        Some(Object::Array(mut annots)) => {
            annots.push(field_id.into());
            doc.new_document
                .get_object_mut(page_id)?
                .as_dict_mut()?
                .set("Annots", annots);
        }
        _ => {
            doc.new_document
                .get_object_mut(page_id)?
                .as_dict_mut()?
                .set("Annots", vec![Object::from(field_id)]);
        }
    }
    Ok(())
}

/// Registers the field in the catalog's `/AcroForm` (creating it if absent),
/// sets `SigFlags` 3 (signatures exist, append-only) and declares the PAdES
/// extension that PDF 1.7 readers need for `ETSI.CAdES.detached`.
fn attach_acroform(
    doc: &mut IncrementalDocument,
    catalog_id: ObjectId,
    field_id: ObjectId,
) -> Result<(), SignError> {
    let existing = doc
        .new_document
        .get_object(catalog_id)?
        .as_dict()?
        .get(b"AcroForm")
        .ok()
        .cloned();
    match existing {
        Some(Object::Reference(form_id)) => {
            doc.opt_clone_object_to_new_document(form_id)?;
            let form = doc.new_document.get_object_mut(form_id)?.as_dict_mut()?;
            push_field(form, field_id);
        }
        Some(Object::Dictionary(mut form)) => {
            push_field(&mut form, field_id);
            doc.new_document
                .get_object_mut(catalog_id)?
                .as_dict_mut()?
                .set("AcroForm", form);
        }
        _ => {
            let form = dictionary! {
                "Fields" => vec![Object::from(field_id)],
                "SigFlags" => 3,
            };
            doc.new_document
                .get_object_mut(catalog_id)?
                .as_dict_mut()?
                .set("AcroForm", form);
        }
    }
    let catalog = doc.new_document.get_object_mut(catalog_id)?.as_dict_mut()?;
    if catalog.get(b"Extensions").is_err() {
        catalog.set(
            "Extensions",
            dictionary! {
                "ESIC" => dictionary! {
                    "BaseVersion" => Object::Name(b"1.7".to_vec()),
                    "ExtensionLevel" => 1,
                },
            },
        );
    }
    Ok(())
}

fn push_field(form: &mut Dictionary, field_id: ObjectId) {
    match form.get_mut(b"Fields").and_then(Object::as_array_mut) {
        Ok(fields) => fields.push(field_id.into()),
        Err(_) => form.set("Fields", vec![Object::from(field_id)]),
    }
    form.set("SigFlags", 3);
}

/// Replaces the placeholders in the serialized increment with the real
/// `/ByteRange` and the CMS signature over everything they exclude.
fn fill_signature(
    mut out: Vec<u8>,
    original_len: usize,
    identity: &SigningIdentity,
    reserve: usize,
    authority: Option<TimestampAuthority<'_>>,
) -> Result<Vec<u8>, SignError> {
    let tail = &out[original_len..];
    let range_at = original_len
        + find(tail, b"/ByteRange")
            .ok_or_else(|| SignError("signature placeholder not found".into()))?;
    let contents_at = range_at
        + find(&out[range_at..], b"/Contents")
            .ok_or_else(|| SignError("signature contents not found".into()))?;
    let open = contents_at
        + find(&out[contents_at..], b"<")
            .ok_or_else(|| SignError("signature contents not found".into()))?;
    let close = open + 1 + 2 * reserve;
    if out.get(close) != Some(&b'>') {
        return err("unexpected signature placeholder layout");
    }

    let after = close + 1;
    let text = format!("0 {open} {after} {}", out.len() - after);
    let start = range_at
        + find(&out[range_at..], b"[").ok_or_else(|| SignError("ByteRange not found".into()))?
        + 1;
    let end =
        start + find(&out[start..], b"]").ok_or_else(|| SignError("ByteRange not found".into()))?;
    let width = end - start;
    if text.len() > width {
        return err("ByteRange does not fit its placeholder");
    }
    out[start..end].copy_from_slice(format!("{text:<width$}").as_bytes());

    let mut hasher = Sha256::new();
    hasher.update(&out[..open]);
    hasher.update(&out[after..]);
    let digest = hasher.finalize();

    let cms = build_cms(identity, &digest, authority)?;
    if cms.len() > reserve {
        return err(format!(
            "the signature ({} bytes) does not fit the {reserve} reserved",
            cms.len()
        ));
    }
    let hex: String = cms.iter().map(|b| format!("{b:02X}")).collect();
    out[open + 1..open + 1 + hex.len()].copy_from_slice(hex.as_bytes());
    Ok(out)
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn build_cms(
    identity: &SigningIdentity,
    digest: &[u8],
    authority: Option<TimestampAuthority<'_>>,
) -> Result<Vec<u8>, SignError> {
    let leaf = &identity.chain[0];
    let sid = SignerIdentifier::IssuerAndSerialNumber(IssuerAndSerialNumber {
        issuer: leaf.tbs_certificate.issuer.clone(),
        serial_number: leaf.tbs_certificate.serial_number.clone(),
    });
    let sha256 = AlgorithmIdentifierOwned {
        oid: OID_SHA256,
        parameters: None,
    };
    // Detached: no eContent, the digest of the PDF ranges is supplied instead.
    let content = EncapsulatedContentInfo {
        econtent_type: OID_DATA,
        econtent: None,
    };

    macro_rules! sign_with {
        ($key:expr, $sig:ty) => {{
            let mut signer =
                SignerInfoBuilder::new($key, sid.clone(), sha256.clone(), &content, Some(digest))
                    .map_err(|e| SignError(format!("CMS signer: {e}")))?;
            signer
                .add_signed_attribute(signing_certificate_v2(leaf)?)
                .map_err(|e| SignError(format!("CMS attribute: {e}")))?;
            let mut builder = SignedDataBuilder::new(&content);
            builder
                .add_digest_algorithm(sha256.clone())
                .map_err(|e| SignError(format!("CMS digest: {e}")))?;
            for cert in &identity.chain {
                builder
                    .add_certificate(CertificateChoices::Certificate(cert.clone()))
                    .map_err(|e| SignError(format!("CMS certificate: {e}")))?;
            }
            builder
                .add_signer_info::<_, $sig>(signer)
                .map_err(|e| SignError(format!("signing failed: {e}")))?;
            builder
                .build()
                .map_err(|e| SignError(format!("CMS: {e}")))?
        }};
    }

    let info: ContentInfo = match &identity.key {
        Key::Rsa(key) => sign_with!(key.as_ref(), rsa::pkcs1v15::Signature),
        Key::P256(key) => sign_with!(key, p256::ecdsa::DerSignature),
    };
    match authority {
        Some(authority) => Ok(add_timestamp(info, authority)?.to_der()?),
        None => Ok(info.to_der()?),
    }
}

const OID_SIGNATURE_TIMESTAMP: ObjectIdentifier =
    ObjectIdentifier::new_unwrap("1.2.840.113549.1.9.16.2.14");

/// Asks the authority to timestamp the signature value and stores the answer
/// as the `signature-time-stamp-token` unsigned attribute (RFC 3161 / CAdES-T).
/// Unsigned attributes are outside what the signature covers, so adding the
/// token afterwards does not disturb it.
fn add_timestamp(
    info: ContentInfo,
    authority: TimestampAuthority<'_>,
) -> Result<ContentInfo, SignError> {
    use x509_tsp::{MessageImprint, TimeStampReq, TimeStampResp, TspVersion};

    let mut data: SignedData = info.content.decode_as()?;
    let mut signer = data
        .signer_infos
        .0
        .iter()
        .next()
        .cloned()
        .ok_or_else(|| SignError("CMS has no signer".into()))?;

    // The imprint is the hash of the signature value, not of the document.
    let imprint = Sha256::digest(signer.signature.as_bytes());
    // Binds the response to this request. Needs to be unpredictable only to the
    // extent that a replayed response must not match: the imprint already ties
    // it to this signature, so a digest of it plus the clock is plenty.
    let nonce = Sha256::digest([imprint.as_slice(), pdf_date().as_bytes()].concat());
    let nonce = der::asn1::Int::new(&nonce[..8].iter().map(|b| b & 0x7F).collect::<Vec<_>>())?;
    let request = TimeStampReq {
        version: TspVersion::V1,
        message_imprint: MessageImprint {
            hash_algorithm: spki::AlgorithmIdentifier {
                oid: OID_SHA256,
                parameters: Some(Any::null()),
            },
            hashed_message: OctetString::new(imprint.to_vec())?,
        },
        req_policy: None,
        nonce: Some(nonce.clone()),
        // Ask for the TSA certificate so a verifier does not have to look it up.
        cert_req: true,
        extensions: None,
    };

    let reply = authority(&request.to_der()?)
        .map_err(|e| SignError(format!("timestamp authority: {e}")))?;
    let reply = TimeStampResp::from_der(&reply)
        .map_err(|e| SignError(format!("timestamp response: {e}")))?;
    use cmpv2::status::PkiStatus;
    if !matches!(
        reply.status.status,
        PkiStatus::Accepted | PkiStatus::GrantedWithMods
    ) {
        return err(format!(
            "the timestamp authority refused the request ({:?})",
            reply.status.status
        ));
    }
    let token = reply
        .time_stamp_token
        .ok_or_else(|| SignError("the timestamp response carries no token".into()))?;
    check_token(&token, &imprint, &nonce)?;

    let mut values = SetOfVec::new();
    values
        .insert(Any::from_der(&token.to_der()?)?)
        .map_err(|e| SignError(format!("ASN.1: {e}")))?;
    let attribute = Attribute {
        oid: OID_SIGNATURE_TIMESTAMP,
        values,
    };
    let mut unsigned = SetOfVec::new();
    unsigned
        .insert(attribute)
        .map_err(|e| SignError(format!("ASN.1: {e}")))?;
    signer.unsigned_attrs = Some(unsigned);

    data.signer_infos = cms::signed_data::SignerInfos(
        SetOfVec::try_from(vec![signer]).map_err(|e| SignError(format!("ASN.1: {e}")))?,
    );
    Ok(ContentInfo {
        content_type: info.content_type,
        content: Any::encode_from(&data)?,
    })
}

/// The token must stamp what we asked it to stamp. A TSA answering for a
/// different imprint or nonce would produce a timestamp that proves nothing
/// about this signature.
fn check_token(
    token: &ContentInfo,
    imprint: &[u8],
    nonce: &der::asn1::Int,
) -> Result<(), SignError> {
    // Read only as far as the TSTInfo. Decoding the whole SignedData would
    // also parse its certificate SET OF strictly, and some authorities repeat
    // their certificate in it; that is harmless here, so it must not be fatal.
    let content_der = token.content.to_der()?;
    let mut reader = der::SliceReader::new(&content_der)?;
    let econtent = reader
        .sequence(|seq| {
            let _version: Any = Any::decode(seq)?;
            let _digest_algorithms: Any = Any::decode(seq)?;
            let content = EncapsulatedContentInfo::decode(seq)?;
            // The certificates and signer infos follow; skip them unparsed.
            while !seq.is_finished() {
                Any::decode(seq)?;
            }
            Ok(content)
        })
        .map_err(|e| SignError(format!("timestamp token: {e}")))?
        .econtent
        .ok_or_else(|| SignError("timestamp token has no TSTInfo".into()))?;
    let octets: OctetString = econtent.decode_as()?;
    let tst = x509_tsp::TstInfo::from_der(octets.as_bytes())
        .map_err(|e| SignError(format!("timestamp TSTInfo: {e}")))?;
    if tst.message_imprint.hashed_message.as_bytes() != imprint {
        return err("the timestamp is for a different signature (imprint mismatch)");
    }
    if tst.nonce.as_ref() != Some(nonce) {
        return err("the timestamp response does not echo the request nonce");
    }
    Ok(())
}

/// `signing-certificate-v2` (RFC 5035): binds the signature to the signer's
/// exact certificate, so swapping in another one with the same name breaks it.
fn signing_certificate_v2(cert: &Certificate) -> Result<Attribute, SignError> {
    let cert_hash = Sha256::digest(cert.to_der()?);
    let issuer_name = cert.tbs_certificate.issuer.to_der()?;
    let serial = cert.tbs_certificate.serial_number.to_der()?;

    // ESSCertIDv2 ::= SEQUENCE { certHash, issuerSerial }  (sha256 is the default)
    let general_name = tlv(0xA4, &issuer_name); // directoryName [4] EXPLICIT Name
    let general_names = tlv(0x30, &general_name);
    let issuer_serial = tlv(0x30, &[general_names, serial].concat());
    let cert_id = tlv(
        0x30,
        &[
            OctetString::new(cert_hash.to_vec())?.to_der()?,
            issuer_serial,
        ]
        .concat(),
    );
    // SigningCertificateV2 ::= SEQUENCE { certs SEQUENCE OF ESSCertIDv2 }
    let value = tlv(0x30, &tlv(0x30, &cert_id));

    let mut values = SetOfVec::new();
    values
        .insert(Any::from_der(&value)?)
        .map_err(|e| SignError(format!("ASN.1: {e}")))?;
    Ok(Attribute {
        oid: OID_SIGNING_CERT_V2,
        values,
    })
}

/// A DER tag-length-value with a definite length.
fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    match content.len() {
        n if n < 0x80 => out.push(n as u8),
        n if n <= 0xFF => out.extend([0x81, n as u8]),
        n => out.extend([0x82, (n >> 8) as u8, n as u8]),
    }
    out.extend_from_slice(content);
    out
}

/// `D:YYYYMMDDHHmmSSZ`, UTC, from the system clock.
fn pdf_date() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil-from-days (Howard Hinnant), proleptic Gregorian.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "D:{year:04}{month:02}{day:02}{:02}{:02}{:02}Z",
        rem / 3_600,
        rem % 3_600 / 60,
        rem % 60
    )
}
