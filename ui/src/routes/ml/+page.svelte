<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { Cpu, RefreshCw, AlertTriangle, Info, CheckCircle, XCircle } from 'lucide-svelte';
  import MetricCard from '$lib/MetricCard.svelte';

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
  let models:    any[]     = $state([]);
  let loading    = $state(true);
  let error      = $state('');
  let kindFilter = $state('');

  const KINDS = ['', 'churn_zscore', 'origin_change', 'path_shortening', 'flap'];

  async function load() {
    loading = true; error = '';
    try {
      const [anomalyRes, modelRes] = await Promise.all([
        api.mlAnomalies(100, kindFilter || undefined),
        api.mlModelStatus().catch(() => ({ models: [] })),
      ]);
      anomalies = (anomalyRes.anomalies ?? []) as Anomaly[];
      models    = modelRes.models ?? [];
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

  const bySeverity = $derived({
    critical: anomalies.filter((a: Anomaly) => a.severity === 'critical').length,
    warn:     anomalies.filter((a: Anomaly) => a.severity === 'warn').length,
    info:     anomalies.filter((a: Anomaly) => a.severity === 'info').length,
  });
</script>

<svelte:head><title>ML Insights — RustyBMP</title></svelte:head>

<div data-testid="page-ml" class="p-6 space-y-6 max-w-6xl mx-auto">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <Cpu size={22} class="text-purple-400" /> ML Anomaly Insights
    </h1>
    <div class="flex items-center gap-2">
      <select data-testid="ml-kind-filter" bind:value={kindFilter} on:change={load}
        class="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded px-3 py-1.5">
        {#each KINDS as k}<option value={k}>{k || 'All kinds'}</option>{/each}
      </select>
      <button data-testid="ml-refresh" on:click={load}
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
    <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
      <MetricCard label="Total Anomalies" value={anomalies.length}       color="blue" />
      <MetricCard label="Critical"        value={bySeverity.critical}    color="red" />
      <MetricCard label="Warnings"        value={bySeverity.warn}        color="yellow" />
      <MetricCard label="Info"            value={bySeverity.info}        color="purple" />
    </div>

    <!-- Model status -->
    {#if models.length > 0}
      <div class="bg-gray-900 border border-gray-800 rounded-xl p-4">
        <h2 class="text-sm font-semibold text-gray-300 mb-3">ML Model Status</h2>
        <div class="grid grid-cols-1 md:grid-cols-2 gap-3">
          {#each models as m}
            <div class="flex items-center gap-3 bg-gray-800/50 rounded-lg px-4 py-3">
              {#if m.ready}
                <CheckCircle size={16} class="text-green-400 flex-shrink-0" />
              {:else}
                <XCircle size={16} class="text-red-400 flex-shrink-0" />
              {/if}
              <div class="min-w-0">
                <div class="text-sm font-medium text-white truncate">{m.model}</div>
                <div class="text-xs text-gray-500 truncate">{m.path}</div>
              </div>
              <span class="ml-auto text-xs px-2 py-0.5 rounded {m.ready ? 'bg-green-900/50 text-green-300' : 'bg-gray-700 text-gray-400'}">
                {m.status}
              </span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
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
