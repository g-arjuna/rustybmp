<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import MetricCard from '$lib/MetricCard.svelte';

  let stats: any   = null;
  let loading      = true;
  let error        = '';

  // Filter test form
  let testPrefix   = '10.0.0.0/8';
  let testPeerAs   = 65000;
  let testAsPath   = '65000 65001 65002';
  let testRpki     = 'valid';
  let testResult: any = null;
  let testing      = false;
  let reloading    = false;
  let reloadMsg    = '';

  onMount(async () => {
    try {
      stats = await api.filterStats();
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  });

  async function runTest() {
    testing    = true;
    testResult = null;
    try {
      testResult = await api.filterTest({
        prefix:  testPrefix,
        peer_as: Number(testPeerAs),
        as_path: testAsPath || undefined,
        rpki:    testRpki   || undefined,
      });
    } catch (e: any) {
      testResult = { error: e.message };
    } finally {
      testing = false;
    }
  }

  async function doReload() {
    reloading = true;
    reloadMsg = '';
    try {
      const r = await api.filterReload();
      reloadMsg = r.status === 'reloaded'
        ? `✓ Reloaded ${r.filters} filter(s) from ${r.path}`
        : `✗ ${r.error}`;
    } catch (e: any) {
      reloadMsg = `✗ ${e.message}`;
    } finally {
      reloading = false;
    }
  }

  const verdictColor: Record<string, string> = {
    accept:          'text-green-400',
    deny:            'text-red-400',
    'default-accept': 'text-yellow-400',
  };
</script>

<div class="p-6 max-w-5xl mx-auto space-y-6">
  <div class="flex items-center justify-between">
    <div>
      <h1 class="text-2xl font-bold text-white">Filter Management</h1>
      <p class="text-gray-400 text-sm mt-1">Test, inspect and hot-reload the BGP route filter chain</p>
    </div>
    <button
      on:click={doReload}
      disabled={reloading}
      class="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 disabled:opacity-50 text-white rounded-lg text-sm font-medium transition-colors"
    >
      {reloading ? 'Reloading…' : '↺ Reload Filters'}
    </button>
  </div>

  {#if reloadMsg}
    <div class="px-4 py-2 rounded-lg text-sm font-mono {reloadMsg.startsWith('✓') ? 'bg-green-900/40 text-green-300' : 'bg-red-900/40 text-red-300'}">
      {reloadMsg}
    </div>
  {/if}

  <!-- Stats cards -->
  {#if loading}
    <div class="grid grid-cols-2 md:grid-cols-3 gap-4">
      {#each Array(3) as _}
        <div class="h-24 bg-gray-800/50 rounded-xl animate-pulse" />
      {/each}
    </div>
  {:else if error}
    <div class="text-red-400 text-sm">{error}</div>
  {:else if stats}
    <div class="grid grid-cols-2 md:grid-cols-3 gap-4">
      <MetricCard label="Active Filters" value={stats.filter_count} color="blue" />
      <MetricCard label="Filter File" value={stats.filter_file} color="purple" />
      <MetricCard label="Metrics Path" value={stats.prometheus_metrics_path} color="green" />
    </div>

    <!-- Counter descriptions -->
    <div class="bg-gray-900/60 border border-gray-700 rounded-xl p-4">
      <h2 class="text-sm font-semibold text-gray-300 mb-3">Prometheus Counters</h2>
      <div class="space-y-1">
        {#each Object.entries(stats.counters ?? {}) as [counter, desc]}
          <div class="flex justify-between text-sm py-1 border-b border-gray-800/50">
            <code class="text-emerald-400 font-mono">{counter}</code>
            <span class="text-gray-400">{desc}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Filter Test Panel -->
  <div class="bg-gray-900/60 border border-gray-700 rounded-xl p-5 space-y-4">
    <h2 class="text-base font-semibold text-white">Test Filter Against Synthetic Route</h2>
    <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
      <div class="space-y-1">
        <label class="text-xs text-gray-400">Prefix</label>
        <input bind:value={testPrefix} class="w-full bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-white font-mono" placeholder="10.0.0.0/8" />
      </div>
      <div class="space-y-1">
        <label class="text-xs text-gray-400">Peer AS</label>
        <input bind:value={testPeerAs} type="number" class="w-full bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-white font-mono" />
      </div>
      <div class="space-y-1">
        <label class="text-xs text-gray-400">AS Path</label>
        <input bind:value={testAsPath} class="w-full bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-white font-mono" placeholder="65000 65001" />
      </div>
      <div class="space-y-1">
        <label class="text-xs text-gray-400">RPKI</label>
        <select bind:value={testRpki} class="w-full bg-gray-800 border border-gray-700 rounded px-3 py-1.5 text-sm text-white">
          <option value="valid">valid</option>
          <option value="invalid">invalid</option>
          <option value="not-found">not-found</option>
          <option value="unknown">unknown</option>
        </select>
      </div>
    </div>
    <button
      on:click={runTest}
      disabled={testing}
      class="px-5 py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 text-white rounded-lg text-sm font-medium transition-colors"
    >
      {testing ? 'Evaluating…' : '▶ Run Test'}
    </button>

    {#if testResult}
      <div class="bg-gray-800/60 rounded-lg p-4 font-mono text-sm space-y-2 mt-2">
        <div class="flex items-center gap-3">
          <span class="text-gray-400">Verdict:</span>
          <span class="font-bold text-lg {verdictColor[testResult.verdict] ?? 'text-white'}">
            {(testResult.verdict ?? 'error').toUpperCase()}
          </span>
        </div>
        {#if testResult.filter_matched}
          <div><span class="text-gray-400">Matched filter:</span> <span class="text-yellow-300">{testResult.filter_matched}</span></div>
        {/if}
        {#if testResult.evaluation_ns}
          <div class="text-gray-500 text-xs">Eval time: {(testResult.evaluation_ns / 1000).toFixed(1)} µs</div>
        {/if}
        {#if testResult.error}
          <div class="text-red-400">{testResult.error}</div>
        {/if}
      </div>
    {/if}
  </div>
</div>
