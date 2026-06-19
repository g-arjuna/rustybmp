use serde::{Deserialize, Serialize};

fn default_bmp_addr() -> String { "0.0.0.0:5000".into() }
fn default_http_addr() -> String { "0.0.0.0:7878".into() }
fn default_db_path() -> String { "runtime/routes.duckdb".into() }
fn default_max_frame() -> u32 { 65535 }
fn default_event_capacity() -> usize { 16384 }
fn default_true() -> bool { true }

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
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            db_path:        default_db_path(),
            in_memory:      false,
            event_capacity: default_event_capacity(),
            checkpoint_secs: 60,
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

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct SpeakerRegistry {
    #[serde(default)]
    pub speakers: Vec<SpeakerEntry>,
}

impl SpeakerRegistry {
    pub fn lookup(&self, addr: &str) -> Option<&SpeakerEntry> {
        self.speakers.iter().find(|e| e.addr == addr)
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
