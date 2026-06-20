<script lang="ts">
  import { onMount } from 'svelte';
  import { BarChart2, RefreshCw } from 'lucide-svelte';

  type StatRow = {
    occurred_at:  string;
    speaker_addr: string;
    peer_addr:    string;
    counter_name: string;
    counter_value: number;
  };

  let stats:  StatRow[] = $state([]);
  let loading = $state(true);
  let error   = $state('');

  async function load() {
    loading = true; error = '';
    try {
      const res = await fetch('/api/bmp/stats?limit=500');
      if (!res.ok) throw new Error(`${res.status}`);
      const j = await res.json() as { stats: StatRow[] };
      stats = j.stats ?? [];
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);

  // Aggregate by counter_name
  $: byCounter = stats.reduce((acc, s) => {
    acc[s.counter_name] = (acc[s.counter_name] ?? 0) + s.counter_value;
    return acc;
  }, {} as Record<string, number>);
  $: counterEntries = Object.entries(byCounter).sort((a, b) => b[1] - a[1]);
  $: maxVal = Math.max(1, ...counterEntries.map(([, v]) => v));

  function fmt(n: number) {
    if (n >= 1_000_000) return `${(n/1_000_000).toFixed(1)}M`;
    if (n >= 1_000)     return `${(n/1_000).toFixed(1)}K`;
    return String(n);
  }
</script>

<svelte:head><title>BMP Stats — RustyBMP</title></svelte:head>

<div class="p-6 space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <BarChart2 size={22} class="text-cyan-400" /> BMP Statistics
    </h1>
    <button on:click={load}
      class="flex items-center gap-1.5 px-3 py-1.5 bg-gray-800 hover:bg-gray-700 text-gray-300 text-sm rounded border border-gray-700">
      <RefreshCw size={13} /> Refresh
    </button>
  </div>

  {#if error}
    <div class="bg-yellow-900/30 border border-yellow-700 text-yellow-300 rounded p-4 text-sm">
      {error} — BMP stats endpoint may not be enabled yet.
    </div>
  {/if}

  {#if loading}
    <div class="text-gray-500 text-sm animate-pulse">Loading BMP stats…</div>
  {:else if counterEntries.length === 0}
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-10 text-center">
      <BarChart2 size={32} class="mx-auto text-gray-700 mb-3" />
      <div class="text-gray-500 text-sm">No BMP statistics recorded yet.</div>
      <div class="text-gray-600 text-xs mt-1">Stats are populated from RFC 7854 Stats Reports sent by BMP speakers.</div>
    </div>
  {:else}
    <!-- Counter bar chart -->
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-5">
      <h2 class="text-sm font-semibold text-gray-300 mb-4">Counter Totals</h2>
      <div class="space-y-2">
        {#each counterEntries as [name, val]}
          {@const pct = Math.round(val / maxVal * 100)}
          <div>
            <div class="flex justify-between text-xs text-gray-400 mb-0.5">
              <span class="font-mono">{name}</span>
              <span class="text-gray-300">{fmt(val)}</span>
            </div>
            <div class="h-2 bg-gray-800 rounded-full overflow-hidden">
              <div class="h-2 bg-cyan-500 rounded-full" style="width:{pct}%"></div>
            </div>
          </div>
        {/each}
      </div>
    </div>

    <!-- Raw table -->
    <div class="bg-gray-900 border border-gray-800 rounded-lg overflow-hidden overflow-x-auto">
      <table class="w-full text-xs text-left">
        <thead>
          <tr class="text-gray-500 border-b border-gray-800 uppercase tracking-wider">
            <th class="px-4 py-3">Time</th>
            <th class="px-4 py-3">Speaker</th>
            <th class="px-4 py-3">Peer</th>
            <th class="px-4 py-3">Counter</th>
            <th class="px-4 py-3 text-right">Value</th>
          </tr>
        </thead>
        <tbody>
          {#each stats.slice(0, 100) as s}
            <tr class="border-b border-gray-800/50 hover:bg-gray-800/30">
              <td class="px-4 py-1.5 font-mono text-gray-500">{new Date(s.occurred_at).toLocaleString()}</td>
              <td class="px-4 py-1.5 font-mono text-gray-400">{s.speaker_addr}</td>
              <td class="px-4 py-1.5 font-mono text-blue-400">{s.peer_addr}</td>
              <td class="px-4 py-1.5 text-gray-300">{s.counter_name}</td>
              <td class="px-4 py-1.5 text-right text-cyan-400 font-mono">{fmt(s.counter_value)}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {/if}
</div>
