-- Bundle A4: Convergence events seed fixture
-- Injects BGP convergence events for testing /api/convergence and the
-- ConvergenceDetector pipeline (Bundle C1).
-- Used by Playwright tests exercising the convergence timeline view.

-- Pull in the baseline speakers/peers/routes
\i tests/seed.sql

-- Convergence events (peer_addr, speaker_addr, prefix_count, elapsed_secs, started_at, occurred_at)
-- Columns: event_id, started_at, eor_at, convergence_ms, speaker_addr, peer_addr,
--          trigger_type, affected_prefixes, recovered_prefixes, unreachable_after
INSERT INTO convergence_events
    (event_id, started_at, eor_at, convergence_ms, speaker_addr, peer_addr,
     trigger_type, affected_prefixes, recovered_prefixes, unreachable_after)
VALUES
  ('conv-001',
   CAST(NOW() AS TIMESTAMP) - INTERVAL '30 minutes', CAST(NOW() AS TIMESTAMP) - INTERVAL '29 minutes 55 seconds',
   4200.0,  '10.0.0.100', '10.0.0.1', 'peer_down',       312,  298,  14),
  ('conv-002',
   CAST(NOW() AS TIMESTAMP) - INTERVAL '1 hour',     CAST(NOW() AS TIMESTAMP) - INTERVAL '59 minutes 59 seconds',
   1100.0,  '10.0.0.100', '10.0.0.2', 'mass_withdraw',   89,   89,   0),
  ('conv-003',
   CAST(NOW() AS TIMESTAMP) - INTERVAL '2 hours',    CAST(NOW() AS TIMESTAMP) - INTERVAL '1 hour 59 minutes 41 seconds',
   18700.0, '10.0.0.200', '10.0.0.3', 'peer_down',       5000, 4991, 9),
  ('conv-004',
   CAST(NOW() AS TIMESTAMP) - INTERVAL '10 minutes', CAST(NOW() AS TIMESTAMP) - INTERVAL '9 minutes 59 seconds',
   80.0,    '10.0.0.200', '10.0.0.4', 'mass_withdraw',   1,    1,    0);
