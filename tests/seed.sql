-- RV8-T2: DuckDB seed fixtures for integration and unit tests.
--
-- Load with:  duckdb :memory: < tests/seed.sql
-- Or in tests: conn.execute_batch(include_str!("../../tests/seed.sql")).unwrap();

-- ── Tables (mirror rbmp_store schema) ────────────────────────────────────────

CREATE TABLE IF NOT EXISTS route_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    peer_as         UINTEGER    NOT NULL,
    rib_type        VARCHAR     NOT NULL,
    action          VARCHAR     NOT NULL,
    prefix          VARCHAR     NOT NULL,
    afi             VARCHAR     NOT NULL,
    origin          VARCHAR,
    as_path         VARCHAR,
    as_path_len     USMALLINT,
    next_hop        VARCHAR,
    local_pref      UINTEGER,
    med             UINTEGER,
    communities     VARCHAR,
    ext_communities VARCHAR,
    large_communities VARCHAR,
    originator_id   VARCHAR,
    atomic_aggregate BOOLEAN DEFAULT false,
    only_to_customer UINTEGER,
    collector_id    VARCHAR
);

CREATE TABLE IF NOT EXISTS peer_events (
    id              UUID        NOT NULL,
    occurred_at     TIMESTAMPTZ NOT NULL,
    speaker_addr    VARCHAR     NOT NULL,
    peer_addr       VARCHAR     NOT NULL,
    peer_as         UINTEGER,
    event_type      VARCHAR     NOT NULL,
    local_as        UINTEGER,
    hold_time       USMALLINT,
    capabilities    VARCHAR,
    reason          VARCHAR,
    collector_id    VARCHAR
);

CREATE TABLE IF NOT EXISTS ml_anomalies (
    id          INTEGER,
    detected_at TIMESTAMPTZ NOT NULL,
    kind        VARCHAR     NOT NULL,
    prefix      VARCHAR,
    peer_addr   VARCHAR,
    score       DOUBLE,
    description VARCHAR,
    severity    VARCHAR
);

CREATE TABLE IF NOT EXISTS convergence_events (
    event_id           VARCHAR     NOT NULL,
    started_at         TIMESTAMPTZ NOT NULL,
    eor_at             TIMESTAMPTZ,
    convergence_ms     DOUBLE,
    speaker_addr       VARCHAR     NOT NULL,
    peer_addr          VARCHAR     NOT NULL,
    trigger_type       VARCHAR,
    affected_prefixes  UINTEGER,
    recovered_prefixes UINTEGER,
    unreachable_after  UINTEGER
);

-- ── Seed data: two BMP speakers, three peers ─────────────────────────────────

-- Speaker 1 (192.0.2.1 = RR1) — two peers up, one peer that flapped
INSERT INTO peer_events
    (id, occurred_at, speaker_addr, peer_addr, peer_as, event_type, local_as, hold_time, capabilities, reason, collector_id)
VALUES
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '120 minutes', '192.0.2.1', '10.0.0.1', 65001, 'peer_up',   NULL, 180, NULL, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '119 minutes', '192.0.2.1', '10.0.0.2', 65002, 'peer_up',   NULL, 180, NULL, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '60  minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_up',   NULL, 180, NULL, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '45  minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_down', NULL, NULL, NULL, 'Hold timer expired', NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '30  minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_up',   NULL, 180, NULL, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '15  minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_down', NULL, NULL, NULL, 'Notification received', NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '5   minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_up',   NULL, 180, NULL, NULL, NULL);

-- Speaker 2 (192.0.2.2 = RR2)
INSERT INTO peer_events
    (id, occurred_at, speaker_addr, peer_addr, peer_as, event_type, local_as, hold_time, capabilities, reason, collector_id)
VALUES
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '90 minutes', '192.0.2.2', '10.0.1.1', 65010, 'peer_up', NULL, 90, NULL, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '89 minutes', '192.0.2.2', '10.0.1.2', 65011, 'peer_up', NULL, 90, NULL, NULL, NULL);

-- Route events: mix of announce/withdraw, RPKI valid/invalid, different ASNs
INSERT INTO route_events
    (id, occurred_at, speaker_addr, peer_addr, peer_as, rib_type, action, prefix, afi, origin, as_path, as_path_len, next_hop, local_pref, med, communities, ext_communities, large_communities, originator_id, atomic_aggregate, only_to_customer, collector_id)
VALUES
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '110 minutes', '192.0.2.1', '10.0.0.1', 65001, 'adj-rib-in-pre', 'announce', '1.2.3.0/24',    'ipv4', 'igp', '65001 65100',       2, '10.0.0.1', 100, NULL, '65100:100',        NULL, NULL, NULL, false, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '109 minutes', '192.0.2.1', '10.0.0.1', 65001, 'adj-rib-in-pre', 'announce', '4.5.6.0/24',    'ipv4', 'igp', '65001 65200 65300',  3, '10.0.0.1', 100, NULL, NULL,               NULL, NULL, NULL, false, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '108 minutes', '192.0.2.1', '10.0.0.1', 65001, 'adj-rib-in-pre', 'announce', '10.0.200.0/24', 'ipv4', 'igp', '65001 65999',        2, '10.0.0.1', 100, NULL, '65999:blackhole',  NULL, NULL, NULL, false, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '107 minutes', '192.0.2.1', '10.0.0.2', 65002, 'adj-rib-in-pre', 'announce', '198.51.100.0/24','ipv4', 'egp', '65002 65100',       2, '10.0.0.2', 200, 50,   '65100:200',        NULL, NULL, NULL, false, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '50  minutes', '192.0.2.1', '10.0.0.3', 65003, 'adj-rib-in-pre', 'announce', '203.0.113.0/24', 'ipv4', 'igp', '65003 65400',        2, '10.0.0.3', 150, NULL, NULL,               NULL, NULL, NULL, false, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '20  minutes', '192.0.2.1', '10.0.0.3', 65003, 'adj-rib-in-pre', 'withdraw', '203.0.113.0/24', 'ipv4', NULL,  NULL,                 NULL, NULL,      NULL, NULL, NULL,               NULL, NULL, NULL, false, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '10  minutes', '192.0.2.1', '10.0.0.3', 65003, 'adj-rib-in-pre', 'announce', '203.0.113.0/24', 'ipv4', 'igp', '65003 65400',        2, '10.0.0.3', 150, NULL, NULL,               NULL, NULL, NULL, false, NULL, NULL),
    -- IPv6
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '100 minutes', '192.0.2.2', '10.0.1.1', 65010, 'adj-rib-in-pre', 'announce', '2001:db8::/32',  'ipv6', 'igp', '65010 65500',       2, '::1',      100, NULL, '65010:1',          NULL, NULL, NULL, false, NULL, NULL),
    (gen_random_uuid(), CAST(NOW() AS TIMESTAMP) - INTERVAL '99  minutes', '192.0.2.2', '10.0.1.2', 65011, 'adj-rib-in-pre', 'announce', '2001:db8:1::/48','ipv6', 'igp', '65011 65500',       2, '::2',      100, NULL, '65010:1',          NULL, NULL, NULL, false, NULL, NULL);

-- ML anomalies: one leak, one hijack
INSERT INTO ml_anomalies
    (detected_at, kind, prefix, peer_addr, score, description, severity)
VALUES
    (CAST(NOW() AS TIMESTAMP) - INTERVAL '30 minutes', 'route_leak', '1.2.3.0/24', '10.0.0.1', 0.87, 'AS65001 unexpectedly propagating AS65100 prefix to downstream', 'high'),
    (CAST(NOW() AS TIMESTAMP) - INTERVAL '15 minutes', 'hijack',     '4.5.6.0/24', '10.0.0.2', 0.92, 'Origin AS changed from 65200 to 65999 — possible BGP hijack',   'critical');

-- Convergence events
INSERT INTO convergence_events
    (event_id, started_at, eor_at, convergence_ms, speaker_addr, peer_addr, trigger_type, affected_prefixes, recovered_prefixes, unreachable_after)
VALUES
    ('evt-001', CAST(NOW() AS TIMESTAMP) - INTERVAL '60 minutes', CAST(NOW() AS TIMESTAMP) - INTERVAL '59 minutes',  45000.0, '192.0.2.1', '10.0.0.3', 'peer_up', 5, 5, 0),
    ('evt-002', CAST(NOW() AS TIMESTAMP) - INTERVAL '30 minutes', CAST(NOW() AS TIMESTAMP) - INTERVAL '29 minutes', 120000.0, '192.0.2.1', '10.0.0.3', 'peer_up', 5, 4, 1),
    ('evt-003', CAST(NOW() AS TIMESTAMP) - INTERVAL '5  minutes', NULL, NULL, '192.0.2.1', '10.0.0.3', 'peer_up', 5, 0, 5);
