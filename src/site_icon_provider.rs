// SPDX-License-Identifier: GPL-3.0-only

use std::{
    fs,
    io::Write,
    net::{IpAddr, Ipv4Addr, Ipv6Addr},
    path::PathBuf,
    time::Duration,
};

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use gtk::glib;
use tempfile::NamedTempFile;
use url::Url;

use crate::{config::DATA_DIR_NAME, util};

const CACHE_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);
const PROVIDER_HOST: &str = "icon.horse";
const CACHE_DIR: &str = "site-icons/icon-horse";

#[async_trait(?Send)]
pub trait SiteIconProvider {
    fn id(&self) -> &'static str;

    async fn fetch(&self, host: &str) -> Result<Vec<u8>>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct IconHorseProvider;

impl IconHorseProvider {
    fn cache_root() -> PathBuf {
        glib::user_cache_dir().join(DATA_DIR_NAME).join(CACHE_DIR)
    }

    fn cache_path(host: &str) -> PathBuf {
        Self::cache_root().join(format!("{}.png", cache_key(host)))
    }

    fn read_cache(host: &str) -> Result<Option<Vec<u8>>> {
        let path = Self::cache_path(host);
        let metadata = match fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(error).with_context(|| format!("failed to inspect {}", path.display()))
            }
        };
        if metadata.len() > u64::from(util::IMAGE_LIMIT as u32)
            || metadata
                .modified()
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_none_or(|age| age > CACHE_TTL)
        {
            return Ok(None);
        }
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        let image = match image::load_from_memory(&bytes) {
            Ok(image) if image.width() == 256 && image.height() == 256 => image,
            _ => return Ok(None),
        };
        if !matches!(
            image.color(),
            image::ColorType::Rgba8 | image::ColorType::Rgba16
        ) {
            return Ok(None);
        }
        Ok(Some(bytes))
    }

    fn write_cache(host: &str, bytes: &[u8]) -> Result<()> {
        let directory = Self::cache_root();
        fs::create_dir_all(&directory)
            .with_context(|| format!("failed to create {}", directory.display()))?;
        let path = Self::cache_path(host);
        let mut temporary = NamedTempFile::new_in(&directory)
            .with_context(|| format!("failed to create cache file in {}", directory.display()))?;
        temporary.write_all(bytes)?;
        temporary.flush()?;
        temporary
            .persist(&path)
            .map_err(|error| error.error)
            .with_context(|| format!("failed to commit {}", path.display()))?;
        Ok(())
    }

    fn endpoint(host: &str) -> Result<Url> {
        let host = normalize_hostname(host)?;
        if is_restricted_host(&host) {
            bail!("external site icon lookup is disabled for local or private hosts");
        }
        let mut endpoint = Url::parse(&format!("https://{PROVIDER_HOST}/"))?;
        endpoint.set_path(&format!("icon/{host}"));
        Ok(endpoint)
    }
}

#[async_trait(?Send)]
impl SiteIconProvider for IconHorseProvider {
    fn id(&self) -> &'static str {
        "icon-horse"
    }

    async fn fetch(&self, host: &str) -> Result<Vec<u8>> {
        let provider_id = self.id();
        let host = normalize_hostname(host)?;
        if is_restricted_host(&host) {
            bail!("external site icon lookup is disabled for local or private hosts");
        }
        if let Some(bytes) = Self::read_cache(&host)? {
            return Ok(bytes);
        }

        let endpoint = Self::endpoint(&host)?;
        let mut response = util::http_client()?.get_async(endpoint.to_string()).await?;
        if !response.status().is_success() {
            bail!("{provider_id} returned HTTP {}", response.status());
        }
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            .map(str::to_ascii_lowercase);
        if content_type
            .as_deref()
            .is_none_or(|value| !value.starts_with("image/"))
        {
            bail!("{provider_id} returned a non-image response");
        }
        let bytes = util::read_bounded(&mut response, util::IMAGE_LIMIT).await?;
        let normalized = util::normalize_icon(bytes).await?;
        Self::write_cache(&host, &normalized)?;
        Ok(normalized)
    }
}

pub fn normalize_hostname(host: &str) -> Result<String> {
    let host = host.trim().trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host.len() > 253
        || host.contains('/')
        || host.contains('?')
        || host.contains('#')
        || host.contains('@')
    {
        bail!("invalid site hostname");
    }
    if host.parse::<IpAddr>().is_ok() {
        return Ok(host);
    }
    if !host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte >= 0x80)
    }) {
        bail!("invalid site hostname");
    }
    Ok(host)
}

fn cache_key(host: &str) -> String {
    // FNV-1a keeps the cache key deterministic across process restarts and
    // Rust versions while avoiding another hashing dependency for this cache.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in host.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

fn is_restricted_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
    {
        return true;
    }
    let Ok(address) = host.parse::<IpAddr>() else {
        return false;
    };
    match address {
        IpAddr::V4(address) => is_restricted_ipv4(address),
        IpAddr::V6(address) => is_restricted_ipv6(address),
    }
}

fn is_restricted_ipv4(address: Ipv4Addr) -> bool {
    address.is_private()
        || address.is_loopback()
        || address.is_link_local()
        || address.is_unspecified()
        || address.is_broadcast()
}

fn is_restricted_ipv6(address: Ipv6Addr) -> bool {
    address.is_loopback()
        || address.is_unspecified()
        || address.is_unique_local()
        || address.is_unicast_link_local()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_public_hostnames() {
        assert_eq!(
            normalize_hostname("WWW.Example.COM.").unwrap(),
            "www.example.com"
        );
        assert!(normalize_hostname("https://example.com").is_err());
        assert!(normalize_hostname("example.com/path").is_err());
    }

    #[test]
    fn blocks_local_and_private_hosts() {
        assert!(is_restricted_host("localhost"));
        assert!(is_restricted_host("printer.local"));
        assert!(is_restricted_host("127.0.0.1"));
        assert!(is_restricted_host("192.168.1.10"));
        assert!(is_restricted_host("::1"));
        assert!(is_restricted_host("fd00::1"));
        assert!(!is_restricted_host("example.com"));
    }

    #[test]
    fn builds_only_icon_horse_https_endpoint() {
        let endpoint = IconHorseProvider::endpoint("Example.com").unwrap();
        assert_eq!(endpoint.scheme(), "https");
        assert_eq!(endpoint.host_str(), Some(PROVIDER_HOST));
        assert_eq!(endpoint.path(), "/icon/example.com");
    }
}
