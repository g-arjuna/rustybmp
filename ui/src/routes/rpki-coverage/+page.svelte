<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import MetricCard from '$lib/MetricCard.svelte';

  let coverage: any = null;
  let loading = true;
  let error   = '';

  onMount(async () => {
    try {
      coverage = await api.rpkiCoverage();
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  });

  $: bars = coverage ? [
    { label: 'Valid',       value: coverage.valid,       pct: coverage.total_prefixes > 0 ? coverage.valid / coverage.total_prefixes * 100 : 0,       color: 'bg-green-500' },
    { label: 'Invalid',     value: coverage.invalid,     pct: coverage.total_prefixes > 0 ? coverage.invalid / coverage.total_prefixes * 100 : 0,     color: 'bg-red-500' },
    { label: 'Not Covered', value: coverage.not_covered, pct: coverage.total_prefixes > 0 ? coverage.not_covered / coverage.total_prefixes * 100 : 0, color: 'bg-gray-600' },
  ] : [];
</script>

<div class="p-6 max-w-4xl mx-auto space-y-6">
  <div>
    <h1 class="text-2xl font-bold text-white">RPKI Coverage Analysis</h1>
    <p class="text-gray-400 text-sm mt-1">What percentage of your active prefixes have ROA coverage?</p>
  </div>

  {#if loading}
    <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
      {#each Array(4) as _}
        <div class="h-24 bg-gray-800/50 rounded-xl animate-pulse" />
      {/each}
    </div>
  {:else if error}
    <div class="text-red-400 text-sm bg-red-900/20 border border-red-800 rounded-lg p-3">{error}</div>
  {:else if coverage}
    <!-- KPI cards -->
    <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
      <MetricCard label="Total Prefixes"    value={coverage.total_prefixes.toLocaleString()} color="blue" />
      <MetricCard label="Coverage"          value={coverage.coverage_pct}   unit="%" color="green" />
      <MetricCard label="ROA Valid"         value={coverage.valid.toLocaleString()}           color="green" />
      <MetricCard label="ROA Invalid"       value={coverage.invalid.toLocaleString()}         color="red" />
    </div>

    <!-- Visual breakdown bar -->
    <div class="bg-gray-900/60 border border-gray-700 rounded-xl p-5 space-y-4">
      <h2 class="text-sm font-semibold text-gray-300">Prefix Breakdown</h2>

      <!-- Stacked bar -->
      <div class="flex h-8 rounded-lg overflow-hidden gap-0.5">
        {#each bars as b}
          {#if b.pct > 0}
            <div
              class="{b.color} transition-all"
              style="width:{b.pct.toFixed(1)}%"
              title="{b.label}: {b.value} ({b.pct.toFixed(1)}%)"
            />
          {/if}
        {/each}
      </div>

      <!-- Legend -->
      <div class="flex flex-wrap gap-5">
        {#each bars as b}
          <div class="flex items-center gap-2">
            <div class="w-3 h-3 rounded-sm {b.color}" />
            <span class="text-sm text-gray-300">{b.label}</span>
            <span class="text-sm font-mono text-white">{b.value.toLocaleString()}</span>
            <span class="text-xs text-gray-500">({b.pct.toFixed(1)}%)</span>
          </div>
        {/each}
      </div>
    </div>

    <!-- Coverage gauge -->
    <div class="bg-gray-900/60 border border-gray-700 rounded-xl p-5">
      <div class="flex items-center justify-between mb-3">
        <h2 class="text-sm font-semibold text-gray-300">Overall ROA Coverage</h2>
        <span class="text-3xl font-bold {coverage.coverage_pct >= 80 ? 'text-green-400' : coverage.coverage_pct >= 50 ? 'text-yellow-400' : 'text-red-400'}">
          {coverage.coverage_pct}%
        </span>
      </div>
      <div class="h-3 bg-gray-800 rounded-full overflow-hidden">
        <div
          class="h-full rounded-full transition-all {coverage.coverage_pct >= 80 ? 'bg-green-500' : coverage.coverage_pct >= 50 ? 'bg-yellow-500' : 'bg-red-500'}"
          style="width:{coverage.coverage_pct}%"
        />
      </div>
      <div class="flex justify-between text-xs text-gray-500 mt-1">
        <span>0%</span>
        <span class="{coverage.coverage_pct >= 80 ? 'text-green-400' : 'text-gray-400'}">80% target</span>
        <span>100%</span>
      </div>
    </div>
  {/if}
</div>
