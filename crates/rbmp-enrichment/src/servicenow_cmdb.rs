/// ServiceNow CMDB enricher (RV9-F2).
///
/// Looks up network devices in ServiceNow CMDB to enrich speaker metadata:
///   ci_name, u_role, u_site
///
/// Also correlates BMP-observed config changes with planned maintenance
/// (open CHG records ±2h).
use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tracing::{debug, warn};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct ServiceNowCmdbConfig {
    /// ServiceNow instance URL
    pub instance_url: String,
    /// Basic auth username
    pub username: String,
    /// Basic auth password
    pub password: String,
    /// Cache TTL override in seconds
    #[serde(default = "default_cache_ttl_secs")]
    pub cache_ttl_secs: u64,
}

fn default_cache_ttl_secs() -> u64 { 900 }

// ── Response models ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmdbDevice {
    pub ci_name: String,
    pub role: String,
    pub site: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeRecord {
    pub number: String,
    pub short_description: String,
    pub state: String,
    pub start_date: String,
    pub end_date: String,
}

#[derive(Debug, Deserialize)]
struct SnowTableResponse {
    result: Vec<serde_json::Value>,
}

// ── Cache entry ──────────────────────────────────────────────────────────────

struct CacheEntry {
    device: Option<CmdbDevice>,
    fetched_at: Instant,
}

// ── Enricher ─────────────────────────────────────────────────────────────────

pub struct ServiceNowCmdbEnricher {
    cfg: ServiceNowCmdbConfig,
    client: Client,
    cache: Mutex<HashMap<IpAddr, CacheEntry>>,
    ttl: Duration,
}

impl ServiceNowCmdbEnricher {
    pub fn new(cfg: ServiceNowCmdbConfig) -> Self {
        let ttl = Duration::from_secs(cfg.cache_ttl_secs);
        Self {
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("SNOW CMDB HTTP client"),
            cfg,
            cache: Mutex::new(HashMap::new()),
            ttl,
        }
    }

    /// Look up a network device by IP in CMDB.
    pub async fn lookup_device(&self, ip: IpAddr) -> Result<Option<CmdbDevice>> {
        // Check cache
        {
            let cache = self.cache.lock().unwrap();
            if let Some(entry) = cache.get(&ip) {
                if entry.fetched_at.elapsed() < self.ttl {
                    debug!(?ip, "SNOW CMDB cache hit");
                    return Ok(entry.device.clone());
                }
            }
        }

        let url = format!(
            "{}/api/now/table/cmdb_ci_network_gear?ip_address={}&sysparm_limit=1",
            self.cfg.instance_url.trim_end_matches('/'),
            ip,
        );

        let resp = self.client
            .get(&url)
            .basic_auth(&self.cfg.username, Some(&self.cfg.password))
            .header("Accept", "application/json")
            .send()
            .await
            .context("SNOW CMDB request failed")?;

        if !resp.status().is_success() {
            warn!(status = %resp.status(), "SNOW CMDB API error");
            return Ok(None);
        }

        let table_resp: SnowTableResponse = resp.json().await.context("SNOW CMDB JSON parse")?;
        let device = table_resp.result.first().map(|v| CmdbDevice {
            ci_name: v["name"].as_str().unwrap_or("").to_string(),
            role: v["u_role"].as_str().unwrap_or("").to_string(),
            site: v["u_site"].as_str().unwrap_or("").to_string(),
        });

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

    /// Find open change records within ±2h of a given timestamp.
    pub async fn find_changes_near(&self, timestamp: &str) -> Result<Vec<ChangeRecord>> {
        let url = format!(
            "{}/api/now/table/change_request?sysparm_query=state=2^start_date<={}^end_date>={}",
            self.cfg.instance_url.trim_end_matches('/'),
            timestamp,
            timestamp,
        );

        let resp = self.client
            .get(&url)
            .basic_auth(&self.cfg.username, Some(&self.cfg.password))
            .header("Accept", "application/json")
            .send()
            .await
            .context("SNOW CHG query failed")?;

        if !resp.status().is_success() {
            return Ok(vec![]);
        }

        let table_resp: SnowTableResponse = resp.json().await?;
        let records = table_resp.result.iter().map(|v| ChangeRecord {
            number: v["number"].as_str().unwrap_or("").to_string(),
            short_description: v["short_description"].as_str().unwrap_or("").to_string(),
            state: v["state"].as_str().unwrap_or("").to_string(),
            start_date: v["start_date"].as_str().unwrap_or("").to_string(),
            end_date: v["end_date"].as_str().unwrap_or("").to_string(),
        }).collect();

        Ok(records)
    }

    pub fn clear_cache(&self) {
        self.cache.lock().unwrap().clear();
    }

    pub fn cache_size(&self) -> usize {
        self.cache.lock().unwrap().len()
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_cfg() -> ServiceNowCmdbConfig {
        ServiceNowCmdbConfig {
            instance_url: "https://test.service-now.com".into(),
            username: "admin".into(),
            password: "password".into(),
            cache_ttl_secs: 900,
        }
    }

    #[test]
    fn enricher_starts_empty() {
        let enricher = ServiceNowCmdbEnricher::new(test_cfg());
        assert_eq!(enricher.cache_size(), 0);
    }

    #[test]
    fn cache_clear() {
        let enricher = ServiceNowCmdbEnricher::new(test_cfg());
        {
            let mut cache = enricher.cache.lock().unwrap();
            cache.insert(
                "10.0.0.1".parse().unwrap(),
                CacheEntry {
                    device: Some(CmdbDevice {
                        ci_name: "router-1".into(),
                        role: "PE".into(),
                        site: "DC1".into(),
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
    fn cmdb_device_serialize() {
        let device = CmdbDevice {
            ci_name: "router-1".into(),
            role: "PE".into(),
            site: "DC1".into(),
        };
        let json = serde_json::to_string(&device).unwrap();
        assert!(json.contains("router-1"));
    }

    #[test]
    fn change_record_fields() {
        let cr = ChangeRecord {
            number: "CHG0012345".into(),
            short_description: "Planned maintenance".into(),
            state: "2".into(),
            start_date: "2025-06-01 10:00:00".into(),
            end_date: "2025-06-01 14:00:00".into(),
        };
        assert_eq!(cr.number, "CHG0012345");
    }
}
