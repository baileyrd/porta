//! A tiny, blocking HTTPS client on top of `ureq` + `rustls` (pure Rust TLS,
//! no system OpenSSL). Every network fetch porta does — installer scripts,
//! release archives — goes through here.

use anyhow::{bail, Context, Result};
use std::io::Read;
use std::time::Duration;
use ureq::tls::{RootCerts, TlsConfig};

const TIMEOUT: Duration = Duration::from_secs(300);

/// Root-of-trust for TLS. Defaults to Mozilla's bundled root list (ureq's
/// own default), which — unlike the OS trust store — can't silently include
/// a corporate TLS-inspecting proxy's root CA. Locked-down corporate
/// machines that legitimately need to trust such a proxy (common on
/// no-admin managed laptops) can opt in with
/// `PORTA_TRUST_SYSTEM_CERTS=1`, which switches to the platform's own
/// certificate store instead.
fn root_certs() -> RootCerts {
    match std::env::var("PORTA_TRUST_SYSTEM_CERTS").as_deref() {
        Ok("1") | Ok("true") => RootCerts::PlatformVerifier,
        _ => RootCerts::WebPki,
    }
}

/// GitHub serves anonymous 404s for private repositories — releases,
/// codeload archives, raw files alike. When `GITHUB_TOKEN` (or `GH_TOKEN`)
/// is set, porta attaches it as a bearer token, but **only** to requests
/// bound for GitHub's own hosts, so the token can never leak to any other
/// endpoint a manifest might name.
fn github_token_for(url: &str) -> Option<String> {
    if !is_github_host(url) {
        return None;
    }
    for var in ["GITHUB_TOKEN", "GH_TOKEN"] {
        if let Ok(token) = std::env::var(var) {
            if !token.is_empty() {
                return Some(token);
            }
        }
    }
    None
}

fn is_github_host(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split(['/', '?']).next().unwrap_or("");
    matches!(
        host,
        "github.com"
            | "codeload.github.com"
            | "raw.githubusercontent.com"
            | "api.github.com"
            | "objects.githubusercontent.com"
            | "release-assets.githubusercontent.com"
    )
}

pub fn fetch_bytes(url: &str) -> Result<Vec<u8>> {
    let tls_config = TlsConfig::builder().root_certs(root_certs()).build();
    let mut request = ureq::get(url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .tls_config(tls_config)
        .build();
    if let Some(token) = github_token_for(url) {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let mut response = request.call().with_context(|| format!("GET {url}"))?;

    let status = response.status();
    if !status.is_success() {
        bail!("GET {url} returned HTTP {status}");
    }

    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .with_context(|| format!("reading response body from {url}"))?;
    Ok(bytes)
}

pub fn fetch_text(url: &str) -> Result<String> {
    let bytes = fetch_bytes(url)?;
    String::from_utf8(bytes).with_context(|| format!("{url} did not return valid UTF-8"))
}

pub fn download_to_file(url: &str, dest: &std::path::Path) -> Result<()> {
    let bytes = fetch_bytes(url)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    std::fs::write(dest, bytes).with_context(|| format!("writing {}", dest.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_scoping_is_github_hosts_only() {
        assert!(is_github_host("https://github.com/o/r/releases/latest"));
        assert!(is_github_host(
            "https://codeload.github.com/o/r/tar.gz/refs/heads/main"
        ));
        assert!(is_github_host(
            "https://raw.githubusercontent.com/o/r/main/install.sh"
        ));
        assert!(is_github_host(
            "https://objects.githubusercontent.com/asset"
        ));

        // Never attach a GitHub token to anything else.
        assert!(!is_github_host("https://sh.rustup.rs"));
        assert!(!is_github_host(
            "https://downloads.claude.ai/claude-code-releases/latest"
        ));
        assert!(!is_github_host("https://evil.example/github.com/"));
        assert!(!is_github_host("https://github.com.evil.example/x"));
        assert!(!is_github_host("http://github.com/o/r")); // plain http: no
    }
}
