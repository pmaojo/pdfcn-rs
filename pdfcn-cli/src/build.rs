use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use clap::Args;
use pdfcn_core::{
    DocumentMetadata, ImageFormat, ImageOptimization, Orientation, PageConfig, PageSize,
    RenderOptions, Theme,
};

/// Cap on a single fetched image, generous for a photo but not for an
/// accidental video/zip a misconfigured URL might point at.
const MAX_REMOTE_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Args)]
pub struct BuildArgs {
    /// Path to the template: .haml, .html, .md, .yaml, .json, .typ or .tex (the extension picks the format)
    template: PathBuf,
    /// Path to the JSON/YAML data file
    #[arg(short, long)]
    data: PathBuf,
    /// Output PDF path
    #[arg(short, long, default_value = "out.pdf")]
    out: PathBuf,
    /// Page size: a4, letter, or "<width>x<height>" in mm
    #[arg(long, default_value = "a4")]
    size: String,
    /// portrait or landscape
    #[arg(long, default_value = "portrait")]
    orientation: String,
    /// Page margin in millimeters
    #[arg(long, default_value_t = 10.0)]
    margin: f32,
    /// Fetch `<img src="http(s)://...">` sources over the network and
    /// embed them (opt-in: pdfcn never does this by default, see NFR-3)
    #[arg(long, default_value_t = false)]
    fetch_remote_images: bool,
    /// Fail instead of warning when the template uses a utility class that
    /// resolves to no CSS (a typo, an unimplemented utility, or a variant
    /// that cannot apply to a PDF such as `hover:` or `md:`).
    #[arg(long, default_value_t = false)]
    strict: bool,
    /// light or dark -- picks shadcn's built-in token table
    #[arg(long, default_value = "light")]
    theme: String,
    /// Repeated on every page (see --skip-first-page). Currently has no
    /// visible effect -- printpdf 0.12.6 doesn't render it yet; kept wired
    /// for forward compatibility (see RenderOptions::header_text).
    #[arg(long)]
    header_text: Option<String>,
    /// Repeated on every page. Same current no-op as --header-text.
    #[arg(long)]
    footer_text: Option<String>,
    /// Appends "Page X of Y" to the footer. Same current no-op.
    #[arg(long, default_value_t = false)]
    show_page_numbers: bool,
    /// Suppresses header/footer/page-numbers on the first page (a cover).
    /// Moot while the above render nothing.
    #[arg(long, default_value_t = false)]
    skip_first_page: bool,
    /// JSON file describing an invoice QR for `%FiscalQR(profile="<id>" ...)`:
    /// its fields and how to validate them, and the text the QR encodes. Lets a
    /// project follow any country's specification without changing pdfcn;
    /// repeat the flag for several. A profile with the id of a built-in one
    /// (e.g. `es-verifactu`) replaces it.
    #[arg(long = "fiscal-profile", value_name = "PATH")]
    fiscal_profiles: Vec<PathBuf>,
    /// Folder of `.svg` files to make available to `%Icon(name="...")`, named
    /// after the file (`receipt.svg` -> `receipt`). Any icon set works: the rest
    /// of Tabler, Lucide, Heroicons, your own artwork. Repeat the flag for
    /// several folders; a name already in use is replaced.
    #[arg(long = "icons-dir", value_name = "DIR")]
    icons_dirs: Vec<PathBuf>,
    /// JPEG-family compression quality, 0.0-1.0 (printpdf's own default:
    /// 0.85). Compression is already on; this only tunes it.
    #[arg(long)]
    image_quality: Option<f32>,
    /// Budget for an embedded image's decoded size (pixels, not the file), e.g.
    /// "300kb" or "2MB": larger images are shrunk to fit. Default "64MB"; a small
    /// budget makes smaller PDFs at the cost of sharpness (printpdf's own
    /// default of "2MB" cuts a photo to about 0.7 megapixels)
    #[arg(long)]
    image_max_size: Option<String>,
    /// Force every embedded image to greyscale before compressing
    #[arg(long, default_value_t = false)]
    image_greyscale: bool,
    /// auto, jpeg, lossless, or raw
    #[arg(long)]
    image_format: Option<String>,
    /// PDF document title metadata
    #[arg(long)]
    title: Option<String>,
    /// PDF document author metadata
    #[arg(long)]
    author: Option<String>,
    /// PDF document subject metadata
    #[arg(long)]
    subject: Option<String>,
    /// Comma-separated PDF document keywords metadata
    #[arg(long)]
    keywords: Option<String>,
    /// SVG side channel for %Vector placeholders (`%Vector(id="...")`):
    /// repeatable `--svg ID=PATH`, where PATH is a .svg file whose text is
    /// embedded under that id (the vector substrate; requires pdfcn-core
    /// built with its `vector` cargo feature).
    #[arg(long = "svg", value_name = "ID=PATH")]
    svg: Vec<String>,
    /// Ola 3: path to an EN 16931/CII invoice XML to embed as a Factur-X
    /// attachment (requires pdfcn-cli/pdfcn-core built with the
    /// `factur-x` cargo feature). Splices the rendered PDF into a
    /// Factur-X-shaped container: embedded `factur-x.xml`, `/AF`/`/Names`
    /// entries, and XMP declaring the profile below.
    #[cfg(feature = "factur-x")]
    #[arg(long, value_name = "PATH")]
    factur_x_xml: Option<PathBuf>,
    /// minimum, basic-wl, basic, en16931 (default), or extended
    #[cfg(feature = "factur-x")]
    #[arg(long, default_value = "en16931")]
    factur_x_profile: String,
    /// Path to a genuine sRGB ICC profile to embed as the PDF/A
    /// `/OutputIntent`. Without this, the Factur-X container still gets
    /// its embedded XML and correct XMP conformance claims, but no
    /// OutputIntent -- see docs/spikes/002-factur-x-embedding.md for why
    /// pdfcn never fabricates one itself.
    #[cfg(feature = "factur-x")]
    #[arg(long, value_name = "PATH")]
    factur_x_icc: Option<PathBuf>,
    /// Sign the PDF (PAdES, `ETSI.CAdES.detached`) with the key and chain in
    /// this PKCS#12 file (`.p12` / `.pfx`). Needs the `sign` cargo feature.
    /// The password is read from the environment (see `--sign-password-env`),
    /// never from the command line, where `ps` and shell history would show it.
    #[cfg(feature = "sign")]
    #[arg(long, value_name = "PATH", conflicts_with_all = ["sign_cert", "sign_key"])]
    sign_p12: Option<PathBuf>,
    /// PEM certificate chain to sign with, signer's own certificate first.
    /// Use with `--sign-key`.
    #[cfg(feature = "sign")]
    #[arg(long, value_name = "PATH", requires = "sign_key")]
    sign_cert: Option<PathBuf>,
    /// PEM private key (PKCS#8, PKCS#1 RSA or SEC1 EC; PKCS#8 may be encrypted).
    #[cfg(feature = "sign")]
    #[arg(long, value_name = "PATH", requires = "sign_cert")]
    sign_key: Option<PathBuf>,
    /// Environment variable holding the PKCS#12 / private key password.
    #[cfg(feature = "sign")]
    #[arg(long, value_name = "VAR", default_value = "PDFCN_SIGN_PASSWORD")]
    sign_password_env: String,
    /// Reason recorded in the signature, e.g. "Invoice issued".
    #[cfg(feature = "sign")]
    #[arg(long)]
    sign_reason: Option<String>,
    /// Location recorded in the signature.
    #[cfg(feature = "sign")]
    #[arg(long)]
    sign_location: Option<String>,
    /// Contact information recorded in the signature.
    #[cfg(feature = "sign")]
    #[arg(long)]
    sign_contact: Option<String>,
    /// Signer name shown by viewers; defaults to the certificate's CN.
    #[cfg(feature = "sign")]
    #[arg(long)]
    sign_name: Option<String>,
    /// RFC 3161 time-stamping authority to ask for a trusted timestamp over
    /// the signature (PAdES baseline-T), e.g. `https://freetsa.org/tsr`. It
    /// keeps the signature checkable after the certificate expires. Signing
    /// fails if the authority cannot be reached: a missing timestamp is never
    /// silently skipped.
    #[cfg(feature = "sign")]
    #[arg(long, value_name = "URL")]
    timestamp_url: Option<String>,
}

/// POSTs an RFC 3161 request to `url` and returns the raw response.
#[cfg(feature = "sign")]
fn ask_timestamp_authority(url: &str, request: &[u8]) -> Result<Vec<u8>, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client
        .post(url)
        .header("Content-Type", "application/timestamp-query")
        .header("Accept", "application/timestamp-reply")
        .body(request.to_vec())
        .send()
        .map_err(|e| format!("{url}: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("{url} answered HTTP {}", response.status()));
    }
    response
        .bytes()
        .map(|b| b.to_vec())
        .map_err(|e| e.to_string())
}

/// Loads the signing identity the `--sign-*` flags describe, or `None` when
/// no signing was asked for.
#[cfg(feature = "sign")]
fn signing_identity(args: &BuildArgs) -> anyhow::Result<Option<pdfcn_core::SigningIdentity>> {
    let password = std::env::var(&args.sign_password_env).ok();
    let hint = || {
        format!(
            "check the password in ${} (it is {})",
            args.sign_password_env,
            if password.is_some() { "set" } else { "not set" }
        )
    };
    if let Some(path) = &args.sign_p12 {
        let data = std::fs::read(path)
            .map_err(|e| anyhow::anyhow!("--sign-p12: cannot read {path:?}: {e}"))?;
        let identity =
            pdfcn_core::SigningIdentity::from_pkcs12(&data, password.as_deref().unwrap_or(""))
                .map_err(|e| anyhow::anyhow!("--sign-p12: {e}; {}", hint()))?;
        return Ok(Some(identity));
    }
    if let (Some(cert), Some(key)) = (&args.sign_cert, &args.sign_key) {
        let cert = std::fs::read_to_string(cert)
            .map_err(|e| anyhow::anyhow!("--sign-cert: cannot read {cert:?}: {e}"))?;
        let key = std::fs::read_to_string(key)
            .map_err(|e| anyhow::anyhow!("--sign-key: cannot read {key:?}: {e}"))?;
        let identity = pdfcn_core::SigningIdentity::from_pem(&cert, &key, password.as_deref())
            .map_err(|e| anyhow::anyhow!("--sign-key: {e}; {}", hint()))?;
        return Ok(Some(identity));
    }
    if args.timestamp_url.is_some() {
        anyhow::bail!(
            "--timestamp-url needs a signature to stamp: add --sign-p12 or --sign-cert/--sign-key"
        );
    }
    Ok(None)
}

#[cfg(feature = "factur-x")]
fn parse_factur_x_profile(s: &str) -> anyhow::Result<pdfcn_core::FacturXProfile> {
    match s.to_ascii_lowercase().replace(['_', ' '], "-").as_str() {
        "minimum" => Ok(pdfcn_core::FacturXProfile::Minimum),
        "basic-wl" => Ok(pdfcn_core::FacturXProfile::BasicWl),
        "basic" => Ok(pdfcn_core::FacturXProfile::Basic),
        "en16931" => Ok(pdfcn_core::FacturXProfile::En16931),
        "extended" => Ok(pdfcn_core::FacturXProfile::Extended),
        other => anyhow::bail!(
            "invalid --factur-x-profile '{other}', expected minimum, basic-wl, basic, en16931, or extended"
        ),
    }
}

/// Fetches one `http(s)://` image URL for `--fetch-remote-images`. Returns
/// `None` (leaving the `<img>` unresolved, same as a missing local file)
/// on any error or an oversized response rather than failing the whole
/// build over one bad image.
fn fetch_remote_image(url: &str) -> Option<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .ok()?;
    let resp = client.get(url).send().ok()?.error_for_status().ok()?;
    if resp
        .content_length()
        .is_some_and(|len| len > MAX_REMOTE_IMAGE_BYTES)
    {
        eprintln!("warning: skipping {url} ({MAX_REMOTE_IMAGE_BYTES} byte limit exceeded)");
        return None;
    }
    let bytes = resp.bytes().ok()?;
    if bytes.len() as u64 > MAX_REMOTE_IMAGE_BYTES {
        eprintln!("warning: skipping {url} ({MAX_REMOTE_IMAGE_BYTES} byte limit exceeded)");
        return None;
    }
    Some(bytes.to_vec())
}

fn parse_size(s: &str) -> anyhow::Result<PageSize> {
    match s.to_ascii_lowercase().as_str() {
        "a4" => Ok(PageSize::A4),
        "letter" => Ok(PageSize::Letter),
        custom => {
            let (w, h) = custom.split_once('x').ok_or_else(|| {
                anyhow::anyhow!("invalid --size '{s}', expected a4, letter, or <width>x<height>")
            })?;
            Ok(PageSize::Custom {
                width_mm: w.parse()?,
                height_mm: h.parse()?,
            })
        }
    }
}

fn parse_orientation(s: &str) -> anyhow::Result<Orientation> {
    match s.to_ascii_lowercase().as_str() {
        "portrait" => Ok(Orientation::Portrait),
        "landscape" => Ok(Orientation::Landscape),
        other => anyhow::bail!("invalid --orientation '{other}', expected portrait or landscape"),
    }
}

fn parse_theme(s: &str) -> anyhow::Result<Theme> {
    match s.to_ascii_lowercase().as_str() {
        "light" => Ok(Theme::light()),
        "dark" => Ok(Theme::dark()),
        other => anyhow::bail!("invalid --theme '{other}', expected light or dark"),
    }
}

fn parse_image_format(s: &str) -> anyhow::Result<ImageFormat> {
    match s.to_ascii_lowercase().as_str() {
        "auto" => Ok(ImageFormat::Auto),
        "jpeg" => Ok(ImageFormat::Jpeg),
        "lossless" => Ok(ImageFormat::Lossless),
        "raw" => Ok(ImageFormat::Raw),
        other => {
            anyhow::bail!("invalid --image-format '{other}', expected auto, jpeg, lossless, or raw")
        }
    }
}

pub fn run(args: BuildArgs) -> anyhow::Result<()> {
    for dir in &args.icons_dirs {
        crate::icons::load_dir(dir)?;
    }
    for path in &args.fiscal_profiles {
        let json = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("--fiscal-profile: cannot read {path:?}: {e}"))?;
        pdfcn_core::register_fiscal_profile(&json)
            .map_err(|e| anyhow::anyhow!("--fiscal-profile {path:?}: {e}"))?;
    }
    // Fail on a bad certificate or password before spending time rendering.
    #[cfg(feature = "sign")]
    let signer = signing_identity(&args)?;
    let page = PageConfig {
        size: parse_size(&args.size)?,
        orientation: parse_orientation(&args.orientation)?,
        margin_mm: args.margin,
    };
    let image_optimization = if args.image_quality.is_some()
        || args.image_max_size.is_some()
        || args.image_greyscale
        || args.image_format.is_some()
    {
        Some(ImageOptimization {
            quality: args.image_quality,
            max_size: args.image_max_size,
            greyscale: args.image_greyscale.then_some(true),
            format: args
                .image_format
                .as_deref()
                .map(parse_image_format)
                .transpose()?,
        })
    } else {
        None
    };
    let keywords = args
        .keywords
        .map(|k| k.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();
    let mut svg_assets = BTreeMap::new();
    for entry in &args.svg {
        let Some((id, path)) = entry.split_once('=') else {
            anyhow::bail!("invalid --svg '{entry}', expected ID=PATH (a .svg file)")
        };
        let source = std::fs::read_to_string(path)
            .map_err(|e| anyhow::anyhow!("--svg {id}: cannot read {path}: {e}"))?;
        svg_assets.insert(id.to_string(), source);
    }
    let options = RenderOptions {
        page,
        theme: parse_theme(&args.theme)?,
        header_text: args.header_text,
        footer_text: args.footer_text,
        show_page_numbers: args.show_page_numbers,
        skip_first_page: args.skip_first_page,
        image_optimization,
        metadata: DocumentMetadata {
            title: args.title,
            author: args.author,
            subject: args.subject,
            keywords,
            producer: None,
        },
        svg_assets,
    };
    let (html, _) = pdfcn_core::render_files_html(&args.template, &args.data, &options.theme)
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    let unresolved = pdfcn_core::unresolved_classes(&html, &options.theme);
    if !unresolved.is_empty() {
        let list = unresolved.iter().cloned().collect::<Vec<_>>().join(", ");
        if args.strict {
            anyhow::bail!("unrecognized utility classes (--strict): {list}");
        }
        eprintln!("warning: unrecognized utility classes, ignored: {list}");
    }
    let fetcher: Option<&pdfcn_core::RemoteImageFetcher> =
        args.fetch_remote_images.then_some(&fetch_remote_image);
    let bytes =
        pdfcn_core::render_files_with_remote_images(&args.template, &args.data, &options, fetcher)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    #[cfg(feature = "factur-x")]
    let bytes = match &args.factur_x_xml {
        Some(xml_path) => {
            let xml = std::fs::read(xml_path)
                .map_err(|e| anyhow::anyhow!("--factur-x-xml: cannot read {xml_path:?}: {e}"))?;
            let icc = args
                .factur_x_icc
                .as_deref()
                .map(std::fs::read)
                .transpose()
                .map_err(|e| anyhow::anyhow!("--factur-x-icc: {e}"))?;
            let profile = parse_factur_x_profile(&args.factur_x_profile)?;
            pdfcn_core::embed_factur_x_invoice(&bytes, &xml, profile, icc.as_deref())
                .map_err(|e| anyhow::anyhow!("{e}"))?
        }
        None => bytes,
    };
    // Signing is the last step: any change to the bytes after it would
    // invalidate the signature.
    #[cfg(feature = "sign")]
    let bytes = match signer {
        Some(identity) => {
            let options = pdfcn_core::SignOptions {
                reason: args.sign_reason.clone(),
                location: args.sign_location.clone(),
                contact_info: args.sign_contact.clone(),
                name: args.sign_name.clone(),
            };
            let signed = match &args.timestamp_url {
                Some(url) => {
                    pdfcn_core::sign_pdf_timestamped(&bytes, &identity, &options, &|req| {
                        ask_timestamp_authority(url, req)
                    })
                }
                None => pdfcn_core::sign_pdf(&bytes, &identity, &options),
            }
            .map_err(|e| anyhow::anyhow!("signing failed: {e}"))?;
            println!(
                "Signed as {}{}",
                identity.subject(),
                if args.timestamp_url.is_some() {
                    " (timestamped)"
                } else {
                    ""
                }
            );
            signed
        }
        None => bytes,
    };
    std::fs::write(&args.out, &bytes)?;
    println!("Wrote {} ({} bytes)", args.out.display(), bytes.len());
    Ok(())
}
