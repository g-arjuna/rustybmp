-- RV8-T2: DuckDB seed fixtures for integration and unit tests.
--
-- Load with:  duckdb :memory: < tests/seed.sql
-- Or in tests: conn.execute_batch(include_str!("../../tests/seed.sql")).unwrap();

-- ── Tables (mirror rbmp_store schema) ────────────────────────────────────────

CREATE TABLE IF NOT EXISTS route_events (
    occurred_at   TIMESTAMPTZ NOT NULL,
    speaker_addr  VARCHAR     NOT NULL,
    peer_addr     VARCHAR     NOT NULL,
    peer_as       UINTEGER    NOT NULL,
    rib_type      VARCHAR     NOT NULL,
    action        VARCHAR     NOT NULL,
    prefix        VARCHAR     NOT NULL,
    afi           VARCHAR     NOT NULL,
    origin        VARCHAR,
    as_path       VARCHAR,
    as_path_len   USMALLINT,
    next_hop      VARCHAR,
    local_pref    UINTEGER,
    med           UINTEGER,
    communities   VARCHAR,
    rpki_validity VARCHAR
);

CREATE TABLE IF NOT EXISTS peer_events (
    occurred_at  TIMESTAMPTZ NOT NULL,
    speaker_addr VARCHAR     NOT NULL,
    peer_addr    VARCHAR     NOT NULL,
    peer_as      UINTEGER,
    event_type   VARCHAR     NOT NULL,
    hold_time    USMALLINT,
    reason       VARCHAR
);

CREATE TABLE IF NOT EXISTS ml_anomalies (
    detected_at  TIMESTAMPTZ NOT NULL,
    kind         VARCHAR     NOT NULL,
    prefix       VARCHAR,
    peer_addr    VARCHAR,
    score        DOUBLE,
    description  VARCHAR,
    severity     VARCHAR
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
INSERT INTO peer_events VALUES
    (NOW() - INTERVAL '120 minutes', '192.0.2.1', '10.0.0.1', 65001, 'peer_up',   180, NULL),
    (NOW() - INTERVAL '119 minutes', '192.0.2.1', '10.0.0.2', 65002, 'peer_up',   180, NULL),
    (NOW() - INTERVAL '60  minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_up',   180, NULL),
    (NOW() - INTERVAL '45  minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_down', NULL, 'Hold timer expired'),
    (NOW() - INTERVAL '30  minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_up',   180, NULL),
    (NOW() - INTERVAL '15  minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_down', NULL, 'Notification received'),
    (NOW() - INTERVAL '5   minutes', '192.0.2.1', '10.0.0.3', 65003, 'peer_up',   180, NULL);

-- Speaker 2 (192.0.2.2 = RR2)
INSERT INTO peer_events VALUES
    (NOW() - INTERVAL '90 minutes', '192.0.2.2', '10.0.1.1', 65010, 'peer_up',   90, NULL),
    (NOW() - INTERVAL '89 minutes', '192.0.2.2', '10.0.1.2', 65011, 'peer_up',   90, NULL);

-- Route events: mix of announce/withdraw, RPKI valid/invalid, different ASNs
INSERT INTO route_events VALUES
    (NOW() - INTERVAL '110 minutes', '192.0.2.1', '10.0.0.1', 65001, 'adj-rib-in-pre',  'announce', '1.2.3.0/24',   'ipv4', 'igp', '65001 65100',      2, '10.0.0.1', 100, NULL, '65100:100',   'valid'),
    (NOW() - INTERVAL '109 minutes', '192.0.2.1', '10.0.0.1', 65001, 'adj-rib-in-pre',  'announce', '4.5.6.0/24',   'ipv4', 'igp', '65001 65200 65300', 3, '10.0.0.1', 100, NULL, NULL,          'not-found'),
    (NOW() - INTERVAL '108 minutes', '192.0.2.1', '10.0.0.1', 65001, 'adj-rib-in-pre',  'announce', '10.0.200.0/24','ipv4', 'igp', '65001 65999',       2, '10.0.0.1', 100, NULL, '65999:blackhole', 'invalid'),
    (NOW() - INTERVAL '107 minutes', '192.0.2.1', '10.0.0.2', 65002, 'adj-rib-in-pre',  'announce', '198.51.100.0/24','ipv4','egp', '65002 65100',      2, '10.0.0.2', 200, 50,   '65100:200',   'valid'),
    (NOW() - INTERVAL '50  minutes', '192.0.2.1', '10.0.0.3', 65003, 'adj-rib-in-pre',  'announce', '203.0.113.0/24','ipv4','igp', '65003 65400',       2, '10.0.0.3', 150, NULL, NULL,          'valid'),
    (NOW() - INTERVAL '20  minutes', '192.0.2.1', '10.0.0.3', 65003, 'adj-rib-in-pre',  'withdraw', '203.0.113.0/24','ipv4', NULL, NULL,                NULL, NULL,     NULL, NULL, NULL,         NULL),
    (NOW() - INTERVAL '10  minutes', '192.0.2.1', '10.0.0.3', 65003, 'adj-rib-in-pre',  'announce', '203.0.113.0/24','ipv4','igp', '65003 65400',       2, '10.0.0.3', 150, NULL, NULL,          'valid'),
    -- IPv6
    (NOW() - INTERVAL '100 minutes', '192.0.2.2', '10.0.1.1', 65010, 'adj-rib-in-pre',  'announce', '2001:db8::/32', 'ipv6', 'igp', '65010 65500',      2, '::1',       100, NULL, '65010:1',     'valid'),
    (NOW() - INTERVAL '99  minutes', '192.0.2.2', '10.0.1.2', 65011, 'adj-rib-in-pre',  'announce', '2001:db8:1::/48','ipv6','igp', '65011 65500',      2, '::2',       100, NULL, '65010:1',     'valid');

-- ML anomalies: one leak, one hijack
INSERT INTO ml_anomalies VALUES
    (NOW() - INTERVAL '30 minutes', 'route_leak', '1.2.3.0/24', '10.0.0.1', 0.87, 'AS65001 unexpectedly propagating AS65100 prefix to downstream', 'high'),
    (NOW() - INTERVAL '15 minutes', 'hijack',     '4.5.6.0/24', '10.0.0.2', 0.92, 'Origin AS changed from 65200 to 65999 — possible BGP hijack',   'critical');

-- Convergence events
INSERT INTO convergence_events VALUES
    ('evt-001', NOW() - INTERVAL '60 minutes', NOW() - INTERVAL '59 minutes',  45000.0, '192.0.2.1', '10.0.0.3', 'peer_up',   5, 5, 0),
    ('evt-002', NOW() - INTERVAL '30 minutes', NOW() - INTERVAL '29 minutes', 120000.0, '192.0.2.1', '10.0.0.3', 'peer_up',   5, 4, 1),
    ('evt-003', NOW() - INTERVAL '5  minutes', NULL,                              NULL, '192.0.2.1', '10.0.0.3', 'peer_up',   5, 0, 5);
