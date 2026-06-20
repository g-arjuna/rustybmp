<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { Cpu, RefreshCw, AlertTriangle, Info } from 'lucide-svelte';

  type Anomaly = {
    detected_at: string;
    kind:        string;
    prefix:      string | null;
    peer_addr:   string | null;
    score:       number | null;
    description: string | null;
    severity:    string | null;
  };

  let anomalies: Anomaly[] = $state([]);
  let loading   = $state(true);
  let error     = $state('');
  let kindFilter = $state('');

  const KINDS = ['', 'churn_zscore', 'origin_change', 'path_shortening', 'flap'];

  async function load() {
    loading = true; error = '';
    try {
      const res = await api.mlAnomalies(100, kindFilter || undefined);
      anomalies = (res.anomalies ?? []) as Anomaly[];
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);

  const SEVERITY_COLORS: Record<string, string> = {
    critical: 'text-red-400 bg-red-900/30 border-red-700/40',
    warn:     'text-yellow-400 bg-yellow-900/30 border-yellow-700/40',
    info:     'text-blue-400 bg-blue-900/30 border-blue-700/40',
  };
  const KIND_LABELS: Record<string, string> = {
    churn_zscore:    'Churn Z-Score',
    origin_change:   'Origin Change',
    path_shortening: 'Path Shortening',
    flap:            'Peer Flap',
  };

  function fmt(dt: string) { return new Date(dt).toLocaleString(); }

  $: bySeverity = {
    critical: anomalies.filter(a => a.severity === 'critical').length,
    warn:     anomalies.filter(a => a.severity === 'warn').length,
    info:     anomalies.filter(a => a.severity === 'info').length,
  };
</script>

<svelte:head><title>ML Insights — RustyBMP</title></svelte:head>

<div class="p-6 space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <Cpu size={22} class="text-purple-400" /> ML Anomaly Insights
    </h1>
    <div class="flex items-center gap-2">
      <select bind:value={kindFilter} on:change={load}
        class="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded px-3 py-1.5">
        {#each KINDS as k}<option value={k}>{k || 'All kinds'}</option>{/each}
      </select>
      <button on:click={load}
        class="flex items-center gap-1.5 px-3 py-1.5 bg-gray-800 hover:bg-gray-700 text-gray-300 text-sm rounded border border-gray-700">
        <RefreshCw size={13} /> Refresh
      </button>
    </div>
  </div>

  {#if error}
    <div class="bg-red-900/30 border border-red-700 text-red-300 rounded p-4 text-sm">{error}</div>
  {/if}

  {#if !loading}
    <!-- Severity summary -->
    <div class="grid grid-cols-3 gap-4">
      {#each [['critical','text-red-400'], ['warn','text-yellow-400'], ['info','text-blue-400']] as [sev, cls]}
        <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
          <div class="text-xs text-gray-500 mb-1 uppercase">{sev}</div>
          <div class="text-3xl font-bold {cls}">{bySeverity[sev as keyof typeof bySeverity]}</div>
        </div>
      {/each}
    </div>
  {/if}

  {#if loading}
    <div class="text-gray-500 text-sm animate-pulse">Loading anomalies…</div>
  {:else if anomalies.length === 0}
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-10 text-center">
      <Cpu size={32} class="mx-auto text-gray-700 mb-3" />
      <div class="text-gray-500 text-sm">No anomalies detected.</div>
      <div class="text-gray-600 text-xs mt-1">Run the Python ML pipeline to populate this table.</div>
    </div>
  {:else}
    <div class="space-y-2">
      {#each anomalies as a}
        {@const sev = a.severity ?? 'info'}
        {@const clz = SEVERITY_COLORS[sev] ?? SEVERITY_COLORS.info}
        <div class="bg-gray-900 border border-gray-800 rounded-lg p-4 flex items-start gap-3">
          <div class="mt-0.5">
            {#if sev === 'critical'}<AlertTriangle size={16} class="text-red-400" />
            {:else}<Info size={16} class="text-yellow-400" />{/if}
          </div>
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2 flex-wrap">
              <span class="text-xs font-semibold px-1.5 py-0.5 rounded border {clz}">
                {KIND_LABELS[a.kind] ?? a.kind}
              </span>
              {#if a.prefix}
                <a href="/prefix/{encodeURIComponent(a.prefix)}"
                   class="font-mono text-xs text-emerald-400 hover:underline">{a.prefix}</a>
              {/if}
              {#if a.peer_addr}
                <a href="/peers/{encodeURIComponent(a.peer_addr)}"
                   class="font-mono text-xs text-blue-400 hover:underline">{a.peer_addr}</a>
              {/if}
              {#if a.score != null}
                <span class="text-xs text-gray-500">score={a.score.toFixed(3)}</span>
              {/if}
              <span class="ml-auto text-xs text-gray-600">{fmt(a.detected_at)}</span>
            </div>
            {#if a.description}
              <div class="mt-1 text-xs text-gray-400">{a.description}</div>
            {/if}
          </div>
        </div>
      {/each}
    </div>
    <div class="text-xs text-gray-600">{anomalies.length} anomalies shown</div>
  {/if}
</div>
