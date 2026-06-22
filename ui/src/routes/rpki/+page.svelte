<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { Shield, RefreshCw, CheckCircle, XCircle, MinusCircle } from 'lucide-svelte';

  type RpkiStats    = Record<string, number>;
  type RpkiAnalysis = { breakdown: RpkiRow[]; per_peer: PeerRow[] };
  type RpkiRow      = { rpki_validity: string; count: number; prefix_count: number };
  type PeerRow      = { peer_addr: string; valid: number; invalid: number; not_found: number; total: number };

  let stats:    RpkiStats    | null = $state(null);
  let analysis: RpkiAnalysis | null = $state(null);
  let loading   = $state(true);
  let error     = $state('');

  async function load() {
    loading = true; error = '';
    try {
      const [s, a] = await Promise.all([
        api.rpkiStats(),
        api.rpkiAnalysis(),
      ]);
      stats    = s as RpkiStats;
      analysis = a as RpkiAnalysis;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);

  const COLORS: Record<string, string> = {
    valid:     'text-emerald-400',
    invalid:   'text-red-400',
    not_found: 'text-yellow-400',
  };
  const ICONS: Record<string, typeof CheckCircle> = {
    valid:     CheckCircle,
    invalid:   XCircle,
    not_found: MinusCircle,
  };
</script>

<svelte:head><title>RPKI Analysis — RustyBMP</title></svelte:head>

<div data-testid="page-rpki" class="p-6 space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <Shield size={22} class="text-emerald-400" /> RPKI Validation
    </h1>
    <button data-testid="rpki-refresh" on:click={load} class="flex items-center gap-1.5 px-3 py-1.5 bg-gray-800 hover:bg-gray-700 text-gray-300 text-sm rounded border border-gray-700">
      <RefreshCw size={13} /> Refresh
    </button>
  </div>

  {#if error}
    <div class="bg-red-900/30 border border-red-700 text-red-300 rounded p-4 text-sm">{error}</div>
  {/if}

  {#if loading}
    <div class="text-gray-500 text-sm animate-pulse">Loading RPKI data…</div>
  {:else if stats}
    <!-- Summary cards -->
    <div data-testid="rpki-summary" class="grid grid-cols-3 gap-4">
      {#each ['valid', 'invalid', 'not_found'] as key}
        {@const val = stats[key] ?? 0}
        {@const total = Object.values(stats).reduce((s, v) => s + v, 0)}
        {@const pct = total > 0 ? Math.round(val / total * 100) : 0}
        <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
          <div class="flex items-center gap-2 text-xs text-gray-500 uppercase tracking-wide mb-2">
            <svelte:component this={ICONS[key]} size={14} class={COLORS[key]} />
            {key.replace('_', ' ')}
          </div>
          <div class="text-3xl font-bold {COLORS[key]}">{val.toLocaleString()}</div>
          <div class="text-xs text-gray-600 mt-1">{pct}% of routes</div>
          <div class="mt-2 h-1 bg-gray-800 rounded-full overflow-hidden">
            <div class="h-1 rounded-full {key === 'valid' ? 'bg-emerald-500' : key === 'invalid' ? 'bg-red-500' : 'bg-yellow-500'}"
                 style="width:{pct}%"></div>
          </div>
        </div>
      {/each}
    </div>

    <!-- Per-peer breakdown -->
    {#if analysis && analysis.per_peer?.length}
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-5">
        <h2 class="text-sm font-semibold text-gray-300 mb-4">Per-Peer RPKI Summary</h2>
        <div class="overflow-x-auto">
          <table data-testid="rpki-peer-table" class="w-full text-xs text-left">
            <thead>
              <tr class="text-gray-500 border-b border-gray-800">
                <th class="pb-2 pr-4">Peer</th>
                <th class="pb-2 pr-4 text-emerald-400">Valid</th>
                <th class="pb-2 pr-4 text-red-400">Invalid</th>
                <th class="pb-2 pr-4 text-yellow-400">Not Found</th>
                <th class="pb-2">Invalid %</th>
              </tr>
            </thead>
            <tbody>
              {#each analysis.per_peer as row}
                {@const pct = row.total > 0 ? Math.round(row.invalid / row.total * 100) : 0}
                <tr class="border-b border-gray-800/50 hover:bg-gray-800/30">
                  <td class="py-1.5 pr-4 font-mono text-blue-400">{row.peer_addr}</td>
                  <td class="py-1.5 pr-4 text-emerald-400">{row.valid}</td>
                  <td class="py-1.5 pr-4 text-red-400">{row.invalid}</td>
                  <td class="py-1.5 pr-4 text-yellow-400">{row.not_found}</td>
                  <td class="py-1.5">
                    <div class="flex items-center gap-2">
                      <div class="w-24 h-1.5 bg-gray-800 rounded-full overflow-hidden">
                        <div class="h-1.5 bg-red-500 rounded-full" style="width:{pct}%"></div>
                      </div>
                      <span class="text-gray-400">{pct}%</span>
                    </div>
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </div>
    {/if}
  {/if}
</div>
