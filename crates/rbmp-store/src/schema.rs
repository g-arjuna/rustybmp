/// DuckDB DDL for the rustybmp schema.
/// All tables use append-only inserts — no updates — to preserve history.

pub const CREATE_TABLES: &str = r#"
-- Route change events (announce / withdraw)
CREATE TABLE IF NOT EXISTS route_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    peer_as         UINTEGER    NOT NULL,
    rib_type        VARCHAR     NOT NULL,
    action          VARCHAR     NOT NULL,  -- 'announce' | 'withdraw'
    prefix          VARCHAR     NOT NULL,
    afi             VARCHAR     NOT NULL,
    origin          VARCHAR,
    as_path         VARCHAR,               -- space-separated ASN list
    as_path_len     USMALLINT,
    next_hop        VARCHAR,
    local_pref      UINTEGER,
    med             UINTEGER,
    communities     VARCHAR,               -- comma-separated
    ext_communities VARCHAR,
    large_communities VARCHAR,
    originator_id   VARCHAR,
    atomic_aggregate BOOLEAN DEFAULT false,
    collector_id    VARCHAR               -- NULL for direct BMP connections (RV3-10)
);

-- BGP peer session events
CREATE TABLE IF NOT EXISTS peer_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    peer_as         UINTEGER,
    event_type      VARCHAR     NOT NULL,  -- 'peer_up' | 'peer_down'
    local_as        UINTEGER,
    hold_time       USMALLINT,
    capabilities    VARCHAR,               -- JSON array
    reason          VARCHAR,
    collector_id    VARCHAR
);

-- BMP speaker sessions
CREATE TABLE IF NOT EXISTS speaker_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    event_type      VARCHAR     NOT NULL,  -- 'speaker_up' | 'speaker_down'
    sys_name        VARCHAR,
    sys_descr       VARCHAR,
    reason          VARCHAR,
    collector_id    VARCHAR
);

-- Statistics snapshots (RFC 7854 + RFC 9972)
CREATE TABLE IF NOT EXISTS stats_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    counter_name    VARCHAR     NOT NULL,
    counter_value   UBIGINT     NOT NULL,
    stat_type       USMALLINT,          -- raw RFC type code
    afi             USMALLINT,          -- NULL for global stats
    safi            UTINYINT            -- NULL for global stats
);

-- EVPN route events (RFC 7432)
CREATE TABLE IF NOT EXISTS evpn_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    peer_as         UINTEGER    NOT NULL,
    action          VARCHAR     NOT NULL,   -- 'announce' | 'withdraw'
    route_type      UTINYINT    NOT NULL,   -- 1-5
    route_type_name VARCHAR     NOT NULL,
    rd              VARCHAR,               -- route distinguisher
    ethernet_tag    UINTEGER,
    mac             VARCHAR,               -- for type 2
    ip              VARCHAR,               -- for type 2/3/4/5
    prefix_len      UTINYINT,              -- for type 5
    mpls_label      UINTEGER,
    esi_hex         VARCHAR                -- 10-byte ESI as hex string
);

-- Indexes for common query patterns
CREATE INDEX IF NOT EXISTS idx_route_events_prefix     ON route_events (prefix);
CREATE INDEX IF NOT EXISTS idx_route_events_peer       ON route_events (peer_addr);
CREATE INDEX IF NOT EXISTS idx_route_events_speaker    ON route_events (speaker_addr);
CREATE INDEX IF NOT EXISTS idx_route_events_time       ON route_events (occurred_at);
CREATE INDEX IF NOT EXISTS idx_route_events_as_path    ON route_events (as_path);
CREATE INDEX IF NOT EXISTS idx_peer_events_peer        ON peer_events (peer_addr);
CREATE INDEX IF NOT EXISTS idx_peer_events_time        ON peer_events (occurred_at);
CREATE INDEX IF NOT EXISTS idx_route_events_collector  ON route_events (collector_id);
CREATE INDEX IF NOT EXISTS idx_peer_events_collector   ON peer_events (collector_id);
"#;
