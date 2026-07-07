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

pub fn fetch_bytes(url: &str) -> Result<Vec<u8>> {
    let tls_config = TlsConfig::builder().root_certs(root_certs()).build();
    let mut response = ureq::get(url)
        .config()
        .timeout_global(Some(TIMEOUT))
        .tls_config(tls_config)
        .build()
        .call()
        .with_context(|| format!("GET {url}"))?;

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
