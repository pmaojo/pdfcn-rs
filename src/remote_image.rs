//! SSRF-guarded remote image fetching, shared by both handlers. `template`/
//! `data` are caller-controlled input on a public endpoint, so an
//! unresolved `<img>` src is exactly the vector that would let a template
//! probe internal services or, on AWS (which Vercel Functions run on), the
//! 169.254.169.254 instance-metadata endpoint -- this module is what keeps
//! that closed off.

use std::collections::BTreeMap;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

/// Cap on a single fetched image and per-request fetch timeout. Shared by
/// both handlers; how many *distinct* sources one request may fetch is
/// each handler's own limit (12 for the single endpoint, 24 across a
/// batch), since that ceiling is inherently per-endpoint.
pub const MAX_REMOTE_IMAGE_BYTES: u64 = 10 * 1024 * 1024;
pub const FETCH_TIMEOUT: Duration = Duration::from_secs(8);

/// Decodes one base64-keyed map (fonts or images), rejecting the request
/// with a client-facing message if any value is malformed.
pub fn decode_base64_map(
    raw: &std::collections::HashMap<String, String>,
    kind: &str,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    let mut out = BTreeMap::new();
    for (key, b64) in raw {
        match STANDARD.decode(b64) {
            Ok(bytes) => {
                out.insert(key.clone(), bytes);
            }
            Err(e) => return Err(format!("invalid base64 for {kind} \"{key}\": {e}")),
        }
    }
    Ok(out)
}

/// True for an IP a template's image URL must never be allowed to reach:
/// loopback, private/link-local ranges, multicast, unspecified, and IPv4
/// mapped into IPv6.
fn is_disallowed_target(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_disallowed_target(IpAddr::V4(mapped));
            }
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || (v6.segments()[0] & 0xfe00) == 0xfc00 // fc00::/7, unique local
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10, link-local
        }
    }
}

/// Resolves host:port, rejecting the target if it has no public IP among
/// its resolved addresses (a hostname can resolve to several; every one of
/// them must be public) or fails to resolve at all.
async fn resolve_public_addr(host: &str, port: u16) -> Result<SocketAddr, &'static str> {
    let addrs = tokio::net::lookup_host((host, port))
        .await
        .map_err(|_| "DNS resolution failed")?
        .collect::<Vec<_>>();
    addrs
        .into_iter()
        .find(|addr| !is_disallowed_target(addr.ip()))
        .ok_or("resolves only to a private/internal address")
}

/// Fetches one http(s) image URL, resolving and validating its host first
/// (see [`is_disallowed_target`]), then requesting that literal validated
/// address with redirects disabled -- a redirect target gets its own
/// validated fetch instead of being followed blindly, closing off
/// SSRF-via-redirect (and, because every hop is re-validated against the
/// same scheme/host/IP rules, this is a fetch-side guard rather than an
/// open redirect: no user-facing Location is ever echoed). The validated
/// address is pinned onto the reqwest client via resolve(), so the
/// connection cannot land anywhere other than the vetted IP -- without the
/// pin, reqwest's internal DNS lookup re-opens a TOCTOU window that a
/// short-TTL rebinding record can slip a private address through. Errors
/// and oversized responses degrade to None (the img renders as a
/// broken-image placeholder) rather than failing the whole request over
/// one bad image, matching a local file that isn't found.
pub async fn fetch_remote_image(url_str: &str) -> Option<Vec<u8>> {
    let mut url = reqwest::Url::parse(url_str).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }

    // Pinned to the last validated origin, rebuilt only when a redirect
    // changes host or port -- not on every hop.
    let mut client: Option<reqwest::Client> = None;
    let mut pinned_host = String::new();
    let mut pinned_port = 0u16;

    // Bounded, not recursive: each hop is itself resolved and validated
    // before being requested, so a chain can't smuggle a private target in
    // past the first hop. The scheme check repeats per hop for the same
    // reason: reqwest only speaks http(s), but saying so here makes the
    // invariant local instead of incidental (and this is a fetch-side
    // guard, not an open redirect: no user-facing Location is echoed).
    for _ in 0..5 {
        if url.scheme() != "http" && url.scheme() != "https" {
            return None;
        }
        let host = url.host_str()?.to_string();
        let port = url.port_or_known_default()?;
        let addr = resolve_public_addr(&host, port).await.ok()?;

        let rebuild = match &client {
            Some(_) => pinned_host != host || pinned_port != port,
            None => true,
        };
        if rebuild {
            // `.resolve` pins the exact address just validated: reqwest
            // connects to `addr` and skips its own DNS lookup entirely.
            // Without this there is a TOCTOU window between the validation
            // above and reqwest's internal resolution -- a short-TTL
            // rebinding record can answer the first lookup with a public
            // IP and the second with 127.0.0.1. The explicit Host header
            // below keeps the request semantically addressed to the
            // original hostname.
            let built = reqwest::Client::builder()
                .timeout(FETCH_TIMEOUT)
                .redirect(reqwest::redirect::Policy::none())
                .resolve(host.as_str(), addr)
                .build()
                .ok()?;
            client = Some(built);
            pinned_host = host;
            pinned_port = port;
        }
        let bound = client.as_ref()?;

        let resp = bound
            .get(url.clone())
            .header(reqwest::header::HOST, format!("{}:{port}", pinned_host))
            .send()
            .await
            .ok()?;

        if resp.status().is_redirection() {
            let location = resp
                .headers()
                .get(reqwest::header::LOCATION)?
                .to_str()
                .ok()?;
            url = url.join(location).ok()?;
            continue;
        }
        if !resp.status().is_success() {
            return None;
        }
        if resp
            .content_length()
            .is_some_and(|len| len > MAX_REMOTE_IMAGE_BYTES)
        {
            return None;
        }
        let bytes = resp.bytes().await.ok()?;
        if bytes.len() as u64 > MAX_REMOTE_IMAGE_BYTES {
            return None;
        }
        return Some(bytes.to_vec());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_private_and_link_local_are_disallowed() {
        for ip in [
            "127.0.0.1",
            "10.0.0.1",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "0.0.0.0",
        ] {
            assert!(
                is_disallowed_target(ip.parse().unwrap()),
                "{ip} must be disallowed"
            );
        }
    }

    #[test]
    fn public_v4_addresses_are_allowed() {
        for ip in ["8.8.8.8", "1.1.1.1", "93.184.216.34"] {
            assert!(
                !is_disallowed_target(ip.parse().unwrap()),
                "{ip} must be allowed"
            );
        }
    }

    #[test]
    fn ipv4_mapped_ipv6_inherits_the_v4_check() {
        // ::ffff:127.0.0.1 -- an IPv6 wrapper around the v4 loopback.
        assert!(is_disallowed_target("::ffff:127.0.0.1".parse().unwrap()));
        assert!(!is_disallowed_target("::ffff:8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn v6_loopback_and_unique_local_are_disallowed() {
        assert!(is_disallowed_target("::1".parse().unwrap()));
        assert!(is_disallowed_target("fc00::1".parse().unwrap()));
        assert!(is_disallowed_target("fe80::1".parse().unwrap()));
    }

    #[test]
    fn decode_base64_map_rejects_malformed_values() {
        let mut raw = std::collections::HashMap::new();
        raw.insert("logo.png".to_string(), "not-base64!!".to_string());
        assert!(decode_base64_map(&raw, "image").is_err());
    }

    #[test]
    fn decode_base64_map_decodes_valid_values() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        let mut raw = std::collections::HashMap::new();
        raw.insert("logo.png".to_string(), STANDARD.encode(b"hello"));
        let decoded = decode_base64_map(&raw, "image").unwrap();
        assert_eq!(decoded["logo.png"], b"hello");
    }
}
