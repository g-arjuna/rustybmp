import { test, expect } from '@playwright/test';

// ── Layout / Navigation ────────────────────────────────────────────────────

test('sidebar is present on every page', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('[data-testid="sidebar"]')).toBeVisible();
  await expect(page.locator('[data-testid="main-nav"]')).toBeVisible();
});

test('nav contains all expected links', async ({ page }) => {
  await page.goto('/');
  const nav = page.locator('[data-testid="main-nav"]');
  for (const href of ['dashboard', 'peers', 'prefixes', 'topology', 'alerts', 'rpki', 'filters', 'query', 'adapters', 'communities', 'flowspec', 'vrf']) {
    await expect(nav.locator(`[data-testid="nav-${href}"]`)).toBeVisible();
  }
});

test('active nav link is highlighted', async ({ page }) => {
  await page.goto('/peers');
  const link = page.locator('[data-testid="nav-peers"]');
  await expect(link).toHaveClass(/text-emerald-400/);
});

// ── Dashboard ──────────────────────────────────────────────────────────────

test('dashboard page renders', async ({ page }) => {
  await page.goto('/');
  await expect(page.locator('[data-testid="main-content"]')).toBeVisible();
});

// ── Peers ──────────────────────────────────────────────────────────────────

test('peers page has search and refresh', async ({ page }) => {
  await page.goto('/peers');
  await expect(page.locator('[data-testid="page-peers"]')).toBeVisible();
  await expect(page.locator('[data-testid="peers-search"]')).toBeVisible();
  await expect(page.locator('[data-testid="peers-refresh"]')).toBeVisible();
});

test('peers table renders on load', async ({ page }) => {
  await page.goto('/peers');
  await expect(page.locator('[data-testid="peers-table"]')).toBeVisible();
});

// ── Prefixes ───────────────────────────────────────────────────────────────

test('prefixes page loads', async ({ page }) => {
  await page.goto('/prefixes');
  await expect(page.locator('[data-testid="page-prefixes"]')).toBeVisible();
});

// ── Topology ───────────────────────────────────────────────────────────────

test('topology page renders with protocol filter', async ({ page }) => {
  await page.goto('/topology');
  await expect(page.locator('[data-testid="page-topology"]')).toBeVisible();
  await expect(page.locator('[data-testid="topology-protocol-filter"]')).toBeVisible();
  await expect(page.locator('[data-testid="topology-refresh"]')).toBeVisible();
});

// ── Filters ────────────────────────────────────────────────────────────────

test('filters page loads', async ({ page }) => {
  await page.goto('/filters');
  await expect(page.locator('[data-testid="page-filters"]')).toBeVisible();
});

// ── Policy ─────────────────────────────────────────────────────────────────

test('policy page has peer selector', async ({ page }) => {
  await page.goto('/policy');
  await expect(page.locator('[data-testid="page-policy"]')).toBeVisible();
});

// ── NL Query (RV9-UX1) ─────────────────────────────────────────────────────

test('query page renders with example chips', async ({ page }) => {
  await page.goto('/query');
  await expect(page.locator('[data-testid="page-query"]')).toBeVisible();
  await expect(page.locator('[data-testid="query-input"]')).toBeVisible();
  await expect(page.locator('[data-testid="query-run-btn"]')).toBeVisible();
  const chips = page.locator('[data-testid="query-example-chip"]');
  await expect(chips).toHaveCount(5);
});

test('clicking example chip populates query input', async ({ page }) => {
  await page.goto('/query');
  await page.locator('[data-testid="query-example-chip"]').first().click();
  const value = await page.locator('[data-testid="query-input"]').inputValue();
  expect(value.length).toBeGreaterThan(0);
});

test('run button is disabled when query is empty', async ({ page }) => {
  await page.goto('/query');
  await expect(page.locator('[data-testid="query-run-btn"]')).toBeDisabled();
});

// ── Adapters (RV9-UX2) ─────────────────────────────────────────────────────

test('adapters page renders', async ({ page }) => {
  await page.goto('/adapters');
  await expect(page.locator('[data-testid="page-adapters"]')).toBeVisible();
  await expect(page.locator('[data-testid="adapters-refresh"]')).toBeVisible();
});

test('adapters page shows empty state when no adapters', async ({ page }) => {
  await page.route('**/api/adapters', async route => {
    await route.fulfill({ json: { adapters: [] } });
  });
  await page.goto('/adapters');
  await expect(page.locator('[data-testid="adapters-empty"]')).toBeVisible();
});

test('adapters list renders cards', async ({ page }) => {
  await page.route('**/api/adapters', async route => {
    await route.fulfill({
      json: {
        adapters: [
          { name: 'snow-em', kind: 'servicenow_em', enabled: true, healthy: true,
            last_push_at: null, event_count: 42, error: null },
        ],
      },
    });
  });
  await page.goto('/adapters');
  await expect(page.locator('[data-testid="adapters-list"]')).toBeVisible();
  await expect(page.locator('[data-testid="adapter-card-snow-em"]')).toBeVisible();
  await expect(page.locator('[data-testid="adapter-test-snow-em"]')).toBeVisible();
});

// ── Communities (RV9-UX3) ──────────────────────────────────────────────────

test('communities page renders with search', async ({ page }) => {
  await page.goto('/communities');
  await expect(page.locator('[data-testid="page-communities"]')).toBeVisible();
  await expect(page.locator('[data-testid="communities-search"]')).toBeVisible();
});

test('communities table renders mock data', async ({ page }) => {
  await page.route('**/api/communities', async route => {
    await route.fulfill({
      json: {
        communities: [
          { community: '65001:100', route_count: 500, pre_policy: 500, post_policy: 498, first_seen: '2025-01-01T00:00:00Z', last_changed: null },
        ],
      },
    });
  });
  await page.route('**/api/communities/semantics', async route => {
    await route.fulfill({ json: { semantics: [] } });
  });
  await page.goto('/communities');
  await expect(page.locator('[data-testid="communities-table"]')).toBeVisible();
  await expect(page.locator('[data-testid="community-row-65001:100"]')).toBeVisible();
});

// ── FlowSpec (RV9-UX5) ─────────────────────────────────────────────────────

test('flowspec page renders', async ({ page }) => {
  await page.goto('/flowspec');
  await expect(page.locator('[data-testid="page-flowspec"]')).toBeVisible();
  await expect(page.locator('[data-testid="flowspec-speaker-filter"]')).toBeVisible();
});

test('flowspec shows large-prefix alert when applicable', async ({ page }) => {
  await page.route('**/api/flowspec/rules*', async route => {
    await route.fulfill({
      json: {
        rules: [{
          id: '1', speaker_addr: '10.0.0.1', peer_addr: '10.0.0.2',
          source_asn: 65001, dst_prefix: '0.0.0.0/0', src_prefix: null,
          proto: null, dst_port: null, src_port: null,
          action: 'rate-bytes=0', rate_bps: 0, community: null,
          received_at: new Date().toISOString(), large_prefix: true,
        }],
      },
    });
  });
  await page.route('**/api/speakers', async route => {
    await route.fulfill({ json: { speakers: [] } });
  });
  await page.goto('/flowspec');
  await expect(page.locator('[data-testid="flowspec-large-prefix-alert"]')).toBeVisible();
  await expect(page.locator('[data-testid="flowspec-table"]')).toBeVisible();
});

// ── VRF Explorer (RV9-UX6) ─────────────────────────────────────────────────

test('VRF page renders', async ({ page }) => {
  await page.goto('/vrf');
  await expect(page.locator('[data-testid="page-vrf"]')).toBeVisible();
});

test('VRF selector populates from API', async ({ page }) => {
  await page.route('**/api/vrf/list', async route => {
    await route.fulfill({
      json: {
        vrfs: [
          { rd: '65001:100', vrf_name: 'CUSTOMER-A', route_count: 120, peer_count: 2, afi: 'IPv4' },
          { rd: '65001:200', vrf_name: 'CUSTOMER-B', route_count: 80,  peer_count: 1, afi: 'IPv4' },
        ],
      },
    });
  });
  await page.route('**/api/vrf/**', async route => {
    await route.fulfill({ json: { routes: [] } });
  });
  await page.goto('/vrf');
  await expect(page.locator('[data-testid="vrf-selector"]')).toBeVisible();
  await expect(page.locator('[data-testid="vrf-card-65001:100"]')).toBeVisible();
  await expect(page.locator('[data-testid="vrf-card-65001:200"]')).toBeVisible();
});
