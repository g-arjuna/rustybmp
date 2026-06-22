-- Bundle A4: Max-prefix capacity seed fixture
-- Injects max-prefix capacity thresholds and a near-limit peer event.
-- Used by Playwright tests that exercise /api/capacity/max-prefix.

-- Pull in the baseline speakers/peers/routes
\i tests/seed.sql

-- Max-prefix capacity thresholds per RFC 4271 §8.2.2 / peer negotiation
-- Columns: updated_at, speaker_addr, peer_addr, peer_as, afi_safi, max_prefix, warning_pct
INSERT INTO peer_max_prefix (updated_at, speaker_addr, peer_addr, peer_as, afi_safi, max_prefix, warning_pct)
VALUES
  (CAST(NOW() AS TIMESTAMP), '10.0.0.100', '10.0.0.1', 65001, 'ipv4-unicast', 750000, 80),
  (CAST(NOW() AS TIMESTAMP), '10.0.0.100', '10.0.0.2', 65002, 'ipv4-unicast', 500000, 80),  -- peer at 99.6% — near limit
  (CAST(NOW() AS TIMESTAMP), '10.0.0.200', '10.0.0.3', 65003, 'ipv4-unicast', 200000, 90),
  (CAST(NOW() AS TIMESTAMP), '10.0.0.200', '10.0.0.4', 65004, 'ipv6-unicast', 100000, 80)
ON CONFLICT (speaker_addr, peer_addr, afi_safi) DO NOTHING;
