-- Bundle A4: Anomaly seed fixture
-- Loads standard seed first, then injects ML anomaly rows.
-- Used by Playwright tests that exercise /api/ml/anomalies.

-- Pull in the baseline speakers/peers/routes
\i tests/seed.sql

-- ML anomaly events
INSERT INTO ml_anomalies (id, detected_at, kind, prefix, peer_addr, score, description, severity)
VALUES
  (1001, NOW() - INTERVAL '5 minutes',  'origin_change',    '203.0.113.0/24', '10.0.0.1', 0.97,
   'Origin ASN changed from 64496 to 64512', 'critical'),
  (1002, NOW() - INTERVAL '12 minutes', 'route_leak',       '192.0.2.0/24',   '10.0.0.2', 0.88,
   'Private prefix propagated to transit peer', 'critical'),
  (1003, NOW() - INTERVAL '3 minutes',  'slow_convergence', '8.8.8.0/24',     '10.0.0.3', 0.61,
   'Convergence 4.2x above baseline', 'warn');
