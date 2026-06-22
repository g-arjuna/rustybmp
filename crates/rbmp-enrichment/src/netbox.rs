/// NetBox DCIM enricher (RV9-F1).
///
/// Enriches the speaker registry with device metadata from NetBox:
///   hostname, site, role, model
///
/// Dual transport: REST API or MCP proxy.
/// Cache TTL: 15 minutes.
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

const CACHE_TTL: Duration = Duration::from_secs(15 * 60);

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct NetboxConfig {
    /// NetBox base URL, e.g. "https://netbox.example.com"
    pub url: String,
    /// API token
    pub token: String,
    /// Cache TTL override in seconds
    #[serde(default = "default_cache_ttl_secs")]
    pub cache_ttl_secs: u64,
}

fn default_cache_ttl_secs() -> u64 { 900 }

// ── Response models ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetboxDevice {
    pub hostname: String,
    pub site: String,
    pub role: String,
    pub model: String,
}

#[derive(Debug, Deserialize)]
struct NetboxApiResponse {
    count: usize,
    results: Vec<NetboxDeviceResult>,
}

#[derive(Debug, Deserialize)]
struct NetboxDeviceResult {
    name: Option<String>,
    site: Option<NetboxRef>,
    device_role: Option<NetboxRef>,
    device_type: Option<NetboxTypeRef>,
}

#[derive(Debug, Deserialize)]
struct NetboxRef {
    name: Option<String>,
    slug: Option<String>,
}

#[derive(Debug, Deserialize)]
struct NetboxTypeRef {
    model: Option<String>,
}

impl NetboxDeviceResult {
    fn to_device(&self) -> NetboxDevice {
        NetboxDevice {
            hostname: self.name.clone().unwrap_or_default(),
            site: self.site.as_ref().and_then(|s| s.name.clone()).unwrap_or_default(),
            role: self.device_role.as_ref().and_then(|r| r.name.clone()).unwrap_or_default(),
            model: self.device_type.as_ref().and_then(|t| t.model.clone()).unwrap_or_default(),
        }
    }
}

// ── Cache entry ──────────────────────────────────────────────────────────────

struct CacheEntry {
    device: Option<NetboxDevice>,
    fetched_at: Instant,
}

// ── Enricher ─────────────────────────────────────────────────────────────────

pub struct NetboxEnricher {
    cfg: NetboxConfig,
    client: Client,
    cache: Mutex<HashMap<IpAddr, CacheEntry>>,
    ttl: Duration,
}

impl NetboxEnricher {
    pub fn new(cfg: NetboxConfig) -> Self {
        let ttl = Duration::from_secs(cfg.cache_ttl_secs);
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("NetBox HTTP client"),
            cfg,
            cache: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Look up device info for a speaker IP.
    pub async fn lookup(&self, ip: IpAddr) -> Result<Option<NetboxDevice>> {
        // Check cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some(entry) = cache.get(&ip) {
                if entry.fetched_at.elapsed() < self.ttl {
                    debug!(?ip, "NetBox cache hit");
                    return Ok(entry.device.clone());
                }
            }
        }

        // Fetch from API
        let url = format!(
            "{}/api/dcim/devices/?primary_ip={}",
            self.cfg.url.trim_end_matches('/'),
            ip
        );
        let resp = self.client
            .get(&url)
            .header("Authorization", format!("Token {}", self.cfg.token))
            .header("Accept", "application/json")
            .send()
            .await
            .context("NetBox API request failed")?;

        if !resp.status().is_success() {
            warn!(status = %resp.status(), "NetBox API error");
            return Ok(None);
        }

        let api_resp: NetboxApiResponse = resp.json().await.context("NetBox JSON parse failed")?;
        let device = api_resp.results.first().map(|d| d.to_device());

        // Update cache
        {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(ip, CacheEntry {
                device: device.clone(),
                fetched_at: Instant::now(),
            });
        }

        Ok(device)
    }

    /// Clear the cache.
    pub fn clear_cache(&self) {
        self.cache.lock().unwrap().clear();
    }

    /// Number of cached entries.
    pub fn cache_size(&self) -> usize {
        self.cache.lock().unwrap().len()
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> NetboxConfig {
        NetboxConfig {
            url: "https://netbox.example.com".into(),
            token: "test-token".into(),
            cache_ttl_secs: 900,
        }
    }

    #[test]
    fn device_result_to_device() {
        let result = NetboxDeviceResult {
            name: Some("router-1".into()),
            site: Some(NetboxRef { name: Some("DC1".into()), slug: Some("dc1".into()) }),
            device_role: Some(NetboxRef { name: Some("PE Router".into()), slug: Some("pe-router".into()) }),
            device_type: Some(NetboxTypeRef { model: Some("ASR-9001".into()) }),
        };
        let device = result.to_device();
        assert_eq!(device.hostname, "router-1");
        assert_eq!(device.site, "DC1");
        assert_eq!(device.role, "PE Router");
        assert_eq!(device.model, "ASR-9001");
    }

    #[test]
    fn device_result_handles_missing_fields() {
        let result = NetboxDeviceResult {
            name: None,
            site: None,
            device_role: None,
            device_type: None,
        };
        let device = result.to_device();
        assert_eq!(device.hostname, "");
        assert_eq!(device.site, "");
    }

    #[test]
    fn enricher_starts_with_empty_cache() {
        let enricher = NetboxEnricher::new(test_cfg());
        assert_eq!(enricher.cache_size(), 0);
    }

    #[test]
    fn cache_clear_works() {
        let enricher = NetboxEnricher::new(test_cfg());
        {
            let mut cache = enricher.cache.lock().unwrap();
            cache.insert(
                "10.0.0.1".parse().unwrap(),
                CacheEntry {
                    device: Some(NetboxDevice {
                        hostname: "test".into(),
                        site: "DC1".into(),
                        role: "PE".into(),
                        model: "ASR".into(),
                    }),
                    fetched_at: Instant::now(),
                },
            );
        }
        assert_eq!(enricher.cache_size(), 1);
        enricher.clear_cache();
        assert_eq!(enricher.cache_size(), 0);
    }

    #[test]
    fn default_cache_ttl() {
        assert_eq!(default_cache_ttl_secs(), 900);
    }
}
