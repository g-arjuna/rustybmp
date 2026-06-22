<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import MetricCard from '$lib/MetricCard.svelte';

  let rows: any[]  = [];
  let loading = true;
  let error   = '';

  async function load() {
    loading = true; error = '';
    try {
      const r = await api.maxPrefixCapacity();
      rows = (r as any).rows ?? [];
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  onMount(load);

  function gaugeColor(pct: number): string {
    if (pct >= 90) return 'bg-red-500';
    if (pct >= 70) return 'bg-amber-400';
    return 'bg-emerald-500';
  }

  function trendStr(v: number | null): string {
    if (v == null) return '—';
    if (Math.abs(v) < 0.5) return 'stable';
    return (v > 0 ? '+' : '') + v.toFixed(1) + '/d';
  }

  function etaStr(v: number | null): string {
    if (v == null) return '—';
    if (v < 1) return '<1d ⚠';
    if (v < 7) return `${v.toFixed(0)}d ⚠`;
    if (v < 30) return `${v.toFixed(0)}d`;
    return '—';
  }

  $: critical = rows.filter((r: any) => r.used_pct >= 90);
  $: warning  = rows.filter((r: any) => r.used_pct >= 70 && r.used_pct < 90);
</script>

<div data-testid="page-capacity" class="p-6 max-w-5xl mx-auto space-y-6">
  <div>
    <h1 class="text-2xl font-bold text-white">Max-Prefix Capacity</h1>
    <p class="text-gray-400 text-sm mt-1">
      RFC 9972 stats type 30 — headroom per peer/AFI-SAFI with linear trend and ETA
    </p>
  </div>

  <!-- Summary KPIs -->
  {#if !loading && !error}
    <div data-testid="capacity-metrics" class="grid grid-cols-2 md:grid-cols-4 gap-4">
      <MetricCard label="Peers Tracked"  value={rows.length.toString()}            color="blue" />
      <MetricCard label="Critical ≥ 90%" value={critical.length.toString()}        color="red" />
      <MetricCard label="Warning 70-90%" value={warning.length.toString()}         color="yellow" />
      <MetricCard label="Healthy"        value={(rows.length - critical.length - warning.length).toString()} color="green" />
    </div>
  {/if}

  <button data-testid="capacity-refresh" on:click={load}
    class="bg-blue-700 hover:bg-blue-600 text-white text-sm px-4 py-1.5 rounded-lg">
    Refresh
  </button>

  {#if loading}
    <div class="space-y-3">
      {#each Array(4) as _}
        <div class="h-20 bg-gray-800/50 rounded-xl animate-pulse" />
      {/each}
    </div>
  {:else if error}
    <div class="text-red-400 text-sm bg-red-900/20 border border-red-800 rounded-lg p-3">{error}</div>
  {:else if rows.length === 0}
    <div class="bg-gray-900/60 border border-gray-700 rounded-xl p-6 text-center">
      <p class="text-gray-400 text-sm">No max-prefix limits configured.</p>
      <p class="text-gray-600 text-xs mt-1">
        Use <code class="bg-gray-800 px-1 rounded">POST /api/capacity/max-prefix</code> to set per-peer limits,
        or wait for BMP stats type 30 data to arrive.
      </p>
    </div>
  {:else}
    <!-- Fuel gauge table -->
    <div class="bg-gray-900/60 border border-gray-700 rounded-xl overflow-hidden">
      <table data-testid="capacity-table" class="w-full text-sm">
        <thead>
          <tr class="bg-gray-800/80 text-gray-400 text-left text-xs">
            <th class="px-4 py-3 font-medium">Peer / AFI-SAFI</th>
            <th class="px-4 py-3 font-medium w-60">Headroom gauge</th>
            <th class="px-3 py-3 font-medium text-right">Used%</th>
            <th class="px-3 py-3 font-medium text-right">Current</th>
            <th class="px-3 py-3 font-medium text-right">Limit</th>
            <th class="px-3 py-3 font-medium text-right">Trend</th>
            <th class="px-3 py-3 font-medium text-right">ETA</th>
          </tr>
        </thead>
        <tbody>
          {#each rows as row}
            <tr class="border-t border-gray-800 hover:bg-gray-800/30">
              <!-- Peer + AFI-SAFI -->
              <td class="px-4 py-3">
                <div class="font-mono text-white text-xs">{row.peer_addr}</div>
                <div class="text-gray-500 text-xs">AS{row.peer_as} · {row.afi_safi}</div>
              </td>

              <!-- Gauge bar -->
              <td class="px-4 py-3">
                <div class="flex items-center gap-2">
                  <div class="flex-1 h-4 bg-gray-700 rounded-full overflow-hidden">
                    <div
                      class="h-full rounded-full transition-all {gaugeColor(row.used_pct)}"
                      style="width: {Math.min(row.used_pct, 100).toFixed(1)}%">
                    </div>
                  </div>
                </div>
              </td>

              <!-- Used % -->
              <td class="px-3 py-3 text-right font-mono text-xs
                {row.used_pct >= 90 ? 'text-red-400' : row.used_pct >= 70 ? 'text-amber-400' : 'text-emerald-400'}">
                {row.used_pct.toFixed(1)}%
              </td>

              <!-- Current / Limit -->
              <td class="px-3 py-3 text-right font-mono text-xs text-gray-300">
                {row.live_count.toLocaleString()}
              </td>
              <td class="px-3 py-3 text-right font-mono text-xs text-gray-500">
                {row.max_prefix.toLocaleString()}
              </td>

              <!-- Trend -->
              <td class="px-3 py-3 text-right text-xs
                {row.trend_per_day != null && row.trend_per_day > 0 ? 'text-amber-300' : 'text-gray-400'}">
                {trendStr(row.trend_per_day)}
              </td>

              <!-- ETA -->
              <td class="px-3 py-3 text-right text-xs
                {row.eta_days != null && row.eta_days < 7 ? 'text-red-400 font-bold' :
                 row.eta_days != null && row.eta_days < 30 ? 'text-amber-400' : 'text-gray-500'}">
                {etaStr(row.eta_days)}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <!-- Critical alert banner -->
    {#if critical.length > 0}
      <div class="bg-red-900/30 border border-red-700 rounded-xl p-4 space-y-1">
        <p class="text-red-300 text-sm font-semibold">⚠ {critical.length} peer(s) near max-prefix limit</p>
        {#each critical as row}
          <p class="text-red-400 text-xs">
            {row.peer_addr} ({row.afi_safi}): {row.used_pct.toFixed(1)}% used
            {#if row.eta_days != null}— exhaustion in ~{etaStr(row.eta_days)}{/if}.
            Consider increasing the limit to {Math.ceil(row.max_prefix * 1.5).toLocaleString()}.
          </p>
        {/each}
      </div>
    {/if}
  {/if}
</div>
