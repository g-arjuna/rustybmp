use serde::{Deserialize, Serialize};

fn default_bmp_addr() -> String { "0.0.0.0:5000".into() }
fn default_http_addr() -> String { "0.0.0.0:7878".into() }
fn default_db_path() -> String { "runtime/routes.duckdb".into() }
fn default_max_frame() -> u32 { 65535 }
fn default_event_capacity() -> usize { 16384 }
fn default_true() -> bool { true }
fn default_jwt_secret() -> String { "change-me-32-byte-minimum-secret!!".into() }
fn default_token_ttl() -> u64 { 86400 }
fn default_retain_days() -> u32 { 90 }
fn default_vault_path()  -> String { "runtime/vault.json".into() }
fn default_nats_server() -> String { "nats://localhost:4222".into() }
fn default_nats_prefix() -> String { "rustybmp".into() }

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub bmp:      BmpConfig,
    #[serde(default)]
    pub http:     HttpConfig,
    #[serde(default)]
    pub store:    StoreConfig,
    #[serde(default)]
    pub log:      LogConfig,
    #[serde(default)]
    pub rpki:     RpkiConfig,
    #[serde(default)]
    pub registry: SpeakerRegistry,
    #[serde(default)]
    pub dns:      DnsConfig,
    #[serde(default)]
    pub proxy:    ProxyConfig,
    #[serde(default)]
    pub kafka:    KafkaConfig,
    #[serde(default)]
    pub auth:     AuthConfig,
    #[serde(default)]
    pub nats:     NatsConfig,
    #[serde(default)]
    pub tls:      TlsConfig,
    #[serde(default)]
    pub ha:       HaConfig,
    /// Path to the YAML filter file. When set, the filter hot-reload watcher
    /// watches this file and applies it on every save.
    /// Example: `filter_file = "config/filters.yaml"`
    pub filter_file: Option<String>,
    /// Path to the credential vault JSON file (RV7-V1).
    /// When absent, defaults to `runtime/vault.json`.
    #[serde(default = "default_vault_path")]
    pub vault_path: String,
}

impl Config {
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&raw)?)
    }

    pub fn default_config() -> Self {
        toml::from_str("").unwrap()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct BmpConfig {
    #[serde(default = "default_bmp_addr")]
    pub listen_addr:    String,
    #[serde(default = "default_max_frame")]
    pub max_frame_bytes: u32,
    /// Drop stats-only messages when backpressure builds
    #[serde(default = "default_true")]
    pub shed_stats_on_pressure: bool,
    /// Write received BMP PDUs to this file (JSONL), optional
    pub archive_path:   Option<String>,
}

impl Default for BmpConfig {
    fn default() -> Self {
        Self {
            listen_addr:    default_bmp_addr(),
            max_frame_bytes: default_max_frame(),
            shed_stats_on_pressure: true,
            archive_path: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HttpConfig {
    #[serde(default = "default_http_addr")]
    pub listen_addr:  String,
    /// Serve the embedded UI (disable to API-only)
    #[serde(default = "default_true")]
    pub serve_ui:     bool,
    /// Optional CORS origin allowlist (empty = allow all)
    #[serde(default)]
    pub cors_origins: Vec<String>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            listen_addr:  default_http_addr(),
            serve_ui:     true,
            cors_origins: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StoreConfig {
    #[serde(default = "default_db_path")]
    pub db_path:          String,
    /// Disable persistent storage (in-memory only)
    #[serde(default)]
    pub in_memory:        bool,
    #[serde(default = "default_event_capacity")]
    pub event_capacity:   usize,
    /// Seconds between DuckDB checkpoint flushes
    #[serde(default)]
    pub checkpoint_secs:  u64,
    /// Delete events older than N days (0 = keep forever)
    #[serde(default = "default_retain_days")]
    pub retain_days:      u32,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            db_path:        default_db_path(),
            in_memory:      false,
            event_capacity: default_event_capacity(),
            checkpoint_secs: 60,
            retain_days:    default_retain_days(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct LogConfig {
    /// "trace" | "debug" | "info" | "warn" | "error"
    #[serde(default)]
    pub level: String,
    /// "json" | "pretty" (default)
    #[serde(default)]
    pub format: String,
}

/// A known BMP speaker with optional metadata
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SpeakerEntry {
    pub addr:     String,
    #[serde(default)]
    pub hostname: String,
    #[serde(default)]
    pub vendor:   String,
    #[serde(default)]
    pub site:     String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SpeakerRegistry {
    #[serde(default)]
    pub speakers: Vec<SpeakerEntry>,
    /// Runtime-registered speakers (via onboarding API) — not persisted to disk.
    #[serde(skip)]
    pub runtime:  std::sync::Mutex<Vec<SpeakerEntry>>,
}

impl Default for SpeakerRegistry {
    fn default() -> Self {
        Self { speakers: Vec::new(), runtime: std::sync::Mutex::new(Vec::new()) }
    }
}

impl Clone for SpeakerRegistry {
    fn clone(&self) -> Self {
        let rt = self.runtime.lock().unwrap().clone();
        Self { speakers: self.speakers.clone(), runtime: std::sync::Mutex::new(rt) }
    }
}

impl SpeakerRegistry {
    pub fn lookup(&self, addr: &str) -> Option<SpeakerEntry> {
        // Check runtime registrations first (override static config)
        if let Ok(rt) = self.runtime.lock() {
            if let Some(e) = rt.iter().find(|e| e.addr == addr) {
                return Some(e.clone());
            }
        }
        self.speakers.iter().find(|e| e.addr == addr).cloned()
    }

    /// Upsert a speaker entry at runtime (onboarding API).
    pub fn upsert(&self, entry: SpeakerEntry) {
        let mut rt = self.runtime.lock().unwrap();
        if let Some(existing) = rt.iter_mut().find(|e| e.addr == entry.addr) {
            *existing = entry;
        } else {
            rt.push(entry);
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RpkiConfig {
    /// Enable RPKI RTR client and route annotation
    #[serde(default)]
    pub enabled: bool,
    /// RTR server address (e.g. "127.0.0.1:3323" for Routinator)
    #[serde(default = "default_rtr_addr")]
    pub rtr_addr: String,
}

fn default_rtr_addr() -> String { "127.0.0.1:3323".into() }
fn default_dns_ttl() -> u64 { 300 }
fn default_proxy_addr() -> String { "0.0.0.0:5001".into() }
fn default_proxy_upstream() -> String { "127.0.0.1:5002".into() }

impl Default for RpkiConfig {
    fn default() -> Self {
        Self { enabled: false, rtr_addr: default_rtr_addr() }
    }
}

/// DNS PTR-lookup enrichment configuration (RV3-4)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct DnsConfig {
    /// Perform PTR lookups for BMP speaker addresses on connect
    #[serde(default)]
    pub enabled:  bool,
    /// TTL in seconds for cached PTR results
    #[serde(default = "default_dns_ttl")]
    pub ttl_secs: u64,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self { enabled: false, ttl_secs: default_dns_ttl() }
    }
}

/// BMP proxy/intercept configuration (RV3-7)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProxyConfig {
    /// Enable the BMP proxy listener
    #[serde(default)]
    pub enabled:      bool,
    /// Address for the proxy to listen on (routers connect here)
    #[serde(default = "default_proxy_addr")]
    pub listen_addr:  String,
    /// Upstream BMP collector to forward all raw bytes to
    #[serde(default = "default_proxy_upstream")]
    pub upstream_addr: String,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            enabled:       false,
            listen_addr:   default_proxy_addr(),
            upstream_addr: default_proxy_upstream(),
        }
    }
}

fn default_kafka_brokers() -> String { "localhost:9092".into() }
fn default_kafka_prefix()  -> String { "rustybmp".into() }

/// Kafka output configuration (RV3-5)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct KafkaConfig {
    /// Enable Kafka output sink
    #[serde(default)]
    pub enabled:      bool,
    /// Comma-separated Kafka broker list
    #[serde(default = "default_kafka_brokers")]
    pub brokers:      String,
    /// Topic prefix (e.g. "rustybmp" → "rustybmp.peer", "rustybmp.unicast_prefix")
    #[serde(default = "default_kafka_prefix")]
    pub topic_prefix: String,
}

impl Default for KafkaConfig {
    fn default() -> Self {
        Self {
            enabled:      false,
            brokers:      default_kafka_brokers(),
            topic_prefix: default_kafka_prefix(),
        }
    }
}

/// JWT authentication configuration (RV4-1)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AuthConfig {
    /// Enable JWT authentication for /api/* endpoints
    #[serde(default)]
    pub enabled:         bool,
    /// HS256 signing secret — must be ≥ 32 bytes in production
    #[serde(default = "default_jwt_secret")]
    pub jwt_secret:      String,
    /// Token TTL in seconds (default: 86400 = 24 hours)
    #[serde(default = "default_token_ttl")]
    pub token_ttl_secs:  u64,
    /// Pre-issued API keys for POST /auth (base64-encoded)
    #[serde(default)]
    pub api_keys:        Vec<String>,
    /// Per-BMP-speaker message rate limit (0 = unlimited)
    #[serde(default)]
    pub rate_limit_msgs_per_sec: u32,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled:                 false,
            jwt_secret:              default_jwt_secret(),
            token_ttl_secs:          default_token_ttl(),
            api_keys:                Vec::new(),
            rate_limit_msgs_per_sec: 0,
        }
    }
}

/// TLS configuration for BMP TCP listener (RV4-1 T2)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    /// Enable TLS on the BMP listener
    #[serde(default)]
    pub enabled:       bool,
    /// Path to PEM-encoded server certificate
    #[serde(default)]
    pub cert_pem:      String,
    /// Path to PEM-encoded private key
    #[serde(default)]
    pub key_pem:       String,
    /// Optional: PEM CA cert for mTLS client verification
    #[serde(default)]
    pub client_ca_pem: String,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled:       false,
            cert_pem:      "certs/server.pem".into(),
            key_pem:       "certs/server.key".into(),
            client_ca_pem: String::new(),
        }
    }
}

fn default_ha_redis()    -> String { "redis://localhost:6379".into() }
fn default_ha_instance() -> String { "core-1".into() }
fn default_ha_lease()    -> u64    { 10 }

/// HA leader election configuration (RV4-7 T1)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HaConfig {
    /// Enable active/passive HA mode
    #[serde(default)]
    pub enabled:     bool,
    /// Redis URL for SETNX lease
    #[serde(default = "default_ha_redis")]
    pub redis_url:   String,
    /// Unique instance identifier
    #[serde(default = "default_ha_instance")]
    pub instance_id: String,
    /// Lease duration in seconds
    #[serde(default = "default_ha_lease")]
    pub lease_secs:  u64,
}

impl Default for HaConfig {
    fn default() -> Self {
        Self {
            enabled:     false,
            redis_url:   default_ha_redis(),
            instance_id: default_ha_instance(),
            lease_secs:  default_ha_lease(),
        }
    }
}

/// NATS output configuration (RV4-7)
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct NatsConfig {
    /// Enable NATS output sink
    #[serde(default)]
    pub enabled:        bool,
    /// NATS server URL (e.g. "nats://localhost:4222")
    #[serde(default = "default_nats_server")]
    pub server:         String,
    /// Subject prefix (e.g. "rustybmp" → "rustybmp.route", "rustybmp.peer")
    #[serde(default = "default_nats_prefix")]
    pub subject_prefix: String,
}

impl Default for NatsConfig {
    fn default() -> Self {
        Self {
            enabled:        false,
            server:         default_nats_server(),
            subject_prefix: default_nats_prefix(),
        }
    }
}
