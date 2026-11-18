//! Fetching remote images the user allowed for one message.
//!
//! The renderer never touches the network; Fuu fetches here, on the user's
//! request, with a privacy-minded client: no cookies, no Referer, a generic
//! User-Agent, limited redirects (http/https only), short timeouts, size caps,
//! raster formats only (declared type and magic bytes must agree), and no
//! requests to loopback, private or link-local addresses.

use std::net::IpAddr;
use std::sync::OnceLock;
use std::time::Duration;

use futures::future::join_all;

/// Most images fetched for one message.
pub const MAX_IMAGES: usize = 60;
/// Per-image size cap.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;

/// (MIME type, bytes) of a fetched image.
pub type Image = (String, Vec<u8>);

fn client() -> Option<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            let tls = crate::tls::client_config().ok()?;
            reqwest::Client::builder()
                .use_preconfigured_tls((*tls).clone())
                .user_agent("Mozilla/5.0")
                .redirect(reqwest::redirect::Policy::custom(|attempt| {
                    let scheme = attempt.url().scheme();
                    if attempt.previous().len() >= 3 || (scheme != "https" && scheme != "http") {
                        attempt.stop()
                    } else {
                        attempt.follow()
                    }
                }))
                .connect_timeout(Duration::from_secs(8))
                .timeout(Duration::from_secs(20))
                .build()
                .ok()
        })
        .as_ref()
}

/// Addresses that must never be reached from mail content.
pub fn is_forbidden_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || (o[0] == 100 && (o[1] & 0xc0) == 64) // CGNAT 100.64/10
                || o[0] == 0
        }
        IpAddr::V6(v6) => {
            let seg = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || (seg[0] & 0xfe00) == 0xfc00 // unique local
                || (seg[0] & 0xffc0) == 0xfe80 // link local
                || v6.to_ipv4_mapped().is_some_and(|v4| is_forbidden_address(IpAddr::V4(v4)))
        }
    }
}

/// Raster type from magic bytes.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

async fn host_allowed(url: &reqwest::Url) -> bool {
    let Some(host) = url.host_str() else { return false };
    if let Ok(ip) = host.trim_start_matches('[').trim_end_matches(']').parse::<IpAddr>() {
        return !is_forbidden_address(ip);
    }
    let port = url.port_or_known_default().unwrap_or(443);
    match tokio::net::lookup_host((host, port)).await {
        Ok(addrs) => {
            let addrs: Vec<_> = addrs.collect();
            !addrs.is_empty() && addrs.iter().all(|a| !is_forbidden_address(a.ip()))
        }
        Err(_) => false,
    }
}

async fn fetch_one(url: &str) -> Option<Image> {
    let client = client()?;
    let url = reqwest::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") || !host_allowed(&url).await {
        return None;
    }
    let mut response = client.get(url).send().await.ok()?;
    if !response.status().is_success() || !host_allowed(response.url()).await {
        return None;
    }
    if response.content_length().is_some_and(|n| n as usize > MAX_IMAGE_BYTES) {
        return None;
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.ok()? {
        bytes.extend_from_slice(&chunk);
        if bytes.len() > MAX_IMAGE_BYTES {
            return None;
        }
    }
    let mime = sniff(&bytes)?;
    Some((mime.to_owned(), bytes))
}

/// Fetch images in order; failures (and anything past [`MAX_IMAGES`]) are `None`.
pub async fn fetch_images(urls: &[String]) -> Vec<Option<Image>> {
    let jobs = urls.iter().take(MAX_IMAGES).map(|u| fetch_one(u));
    let mut results = join_all(jobs).await;
    results.resize(urls.len(), None);
    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbids_local_and_private_addresses() {
        for bad in ["127.0.0.1", "10.1.2.3", "192.168.0.1", "172.16.5.4", "169.254.169.254", "0.0.0.0", "100.64.0.1", "::1", "fe80::1", "fd00::1", "::ffff:127.0.0.1"] {
            assert!(is_forbidden_address(bad.parse().unwrap()), "{bad}");
        }
        for good in ["8.8.8.8", "203.0.114.1", "2606:4700::1111"] {
            assert!(!is_forbidden_address(good.parse().unwrap()), "{good}");
        }
    }

    #[test]
    fn sniffs_raster_formats_only() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(sniff(&[0xff, 0xd8, 0xff, 0xe0]), Some("image/jpeg"));
        assert_eq!(sniff(b"GIF89a.."), Some("image/gif"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"<svg xmlns=..."), None);
        assert_eq!(sniff(b"<html>"), None);
    }

    #[tokio::test]
    async fn refuses_local_targets_without_connecting() {
        let out = fetch_images(&["http://127.0.0.1:9/x.png".into(), "http://localhost/x.png".into(), "file:///C:/x.png".into()]).await;
        assert_eq!(out, vec![None, None, None]);
    }
}
