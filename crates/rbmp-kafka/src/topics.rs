/// Well-known topic name suffixes.  Final topic = prefix + suffix.
/// Mirrors the OpenBMP topic taxonomy for compatibility.
pub const TOPIC_ROUTER:    &str = "router";
pub const TOPIC_PEER:      &str = "peer";
pub const TOPIC_UNICAST:   &str = "unicast_prefix";
pub const TOPIC_EVPN:      &str = "evpn";
pub const TOPIC_BGPLS:     &str = "ls_node";
pub const TOPIC_STATS:     &str = "bmp_stat";
pub const TOPIC_RAW:       &str = "bmp_raw";

/// Build a fully-qualified topic name from a prefix and suffix.
pub fn topic(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        suffix.to_string()
    } else {
        format!("{}.{}", prefix, suffix)
    }
}
