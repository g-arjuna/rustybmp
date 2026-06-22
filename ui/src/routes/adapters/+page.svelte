<script lang="ts">
  import { onMount } from 'svelte';
  import { Plug, RefreshCw, CheckCircle, XCircle, Play, AlertTriangle } from 'lucide-svelte';

  interface AdapterStatus {
    name:         string;
    kind:         string;
    enabled:      boolean;
    healthy:      boolean;
    last_push_at: string | null;
    event_count:  number;
    error:        string | null;
  }

  let adapters = $state<AdapterStatus[]>([]);
  let loading  = $state(true);
  let testing  = $state<Record<string, boolean>>({});
  let testResults = $state<Record<string, { ok: boolean; message: string }>>({});

  async function load() {
    loading = true;
    try {
      const res = await fetch('/api/adapters');
      const j   = await res.json() as { adapters: AdapterStatus[] };
      adapters  = j.adapters ?? [];
    } catch {
      adapters = [];
    } finally {
      loading = false;
    }
  }

  async function testAdapter(name: string) {
    testing  = { ...testing,  [name]: true };
    testResults = { ...testResults, [name]: { ok: false, message: '' } };
    try {
      const res = await fetch(`/api/adapters/${encodeURIComponent(name)}/test`, { method: 'POST' });
      const j   = await res.json() as { ok: boolean; message: string };
      testResults = { ...testResults, [name]: j };
    } catch (e) {
      testResults = { ...testResults, [name]: { ok: false, message: String(e) } };
    } finally {
      testing = { ...testing, [name]: false };
    }
  }

  onMount(load);

  const KIND_LABELS: Record<string, string> = {
    servicenow_em: 'ServiceNow EM',
    webhook_slack:  'Slack Webhook',
    webhook_pd:     'PagerDuty',
    webhook_opsgenie: 'OpsGenie',
    webhook_teams:  'MS Teams',
    elasticsearch:  'Elasticsearch',
    splunk_hec:     'Splunk HEC',
  };

  function kindLabel(kind: string) { return KIND_LABELS[kind] ?? kind; }
</script>

<svelte:head><title>Output Adapters — RustyBMP</title></svelte:head>

<div data-testid="page-adapters" class="p-6 space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <Plug size={22} class="text-purple-400" /> Output Adapters
    </h1>
    <button
      data-testid="adapters-refresh"
      onclick={load}
      class="p-2 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-gray-100 transition-colors"
      title="Refresh"
    >
      <RefreshCw size={15} />
    </button>
  </div>

  <p class="text-sm text-gray-500">
    Configured output adapters forward anomaly events to external systems.
    Use the <span class="font-mono text-gray-400">Test</span> button to verify connectivity.
  </p>

  {#if loading}
    <p class="text-gray-500 text-sm animate-pulse">Loading adapters…</p>
  {:else if adapters.length === 0}
    <div data-testid="adapters-empty" class="bg-gray-900 border border-gray-800 rounded-xl p-10 text-center space-y-3">
      <Plug size={36} class="text-gray-700 mx-auto" />
      <p class="text-gray-500 text-sm">No output adapters configured.</p>
      <p class="text-xs text-gray-600">
        Add adapters to <span class="font-mono">config/rustybmp.toml</span> under the
        <span class="font-mono">[outputs]</span> section.
      </p>
    </div>
  {:else}
    <div data-testid="adapters-list" class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
      {#each adapters as adapter}
        <div data-testid="adapter-card-{adapter.name}"
             class="bg-gray-900 border border-gray-800 rounded-xl p-5 space-y-4 flex flex-col">
          <!-- Header -->
          <div class="flex items-start justify-between gap-2">
            <div>
              <h2 class="text-base font-semibold text-gray-100">{adapter.name}</h2>
              <span class="text-xs text-gray-500">{kindLabel(adapter.kind)}</span>
            </div>
            <div class="flex items-center gap-1.5 mt-0.5">
              {#if !adapter.enabled}
                <span class="text-xs px-2 py-0.5 rounded bg-gray-700 text-gray-400">disabled</span>
              {:else if adapter.healthy}
                <CheckCircle size={16} class="text-emerald-400" />
                <span class="text-xs text-emerald-400">healthy</span>
              {:else}
                <XCircle size={16} class="text-red-400" />
                <span class="text-xs text-red-400">unhealthy</span>
              {/if}
            </div>
          </div>

          <!-- Stats -->
          <dl class="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
            <dt class="text-gray-500">Events sent</dt>
            <dd class="text-gray-300 font-mono text-right">{adapter.event_count.toLocaleString()}</dd>
            <dt class="text-gray-500">Last push</dt>
            <dd class="text-gray-300 font-mono text-right">
              {adapter.last_push_at ? new Date(adapter.last_push_at).toLocaleTimeString() : '—'}
            </dd>
          </dl>

          <!-- Error -->
          {#if adapter.error}
            <div class="flex items-start gap-1.5 text-xs text-yellow-300 bg-yellow-900/20 border border-yellow-700/40 rounded p-2">
              <AlertTriangle size={12} class="mt-0.5 flex-shrink-0" />
              <span class="break-all">{adapter.error}</span>
            </div>
          {/if}

          <!-- Test result -->
          {#if testResults[adapter.name]}
            {@const r = testResults[adapter.name]}
            <div class="text-xs rounded p-2 {r.ok ? 'bg-emerald-900/20 border border-emerald-700/40 text-emerald-300' : 'bg-red-900/20 border border-red-700/40 text-red-300'}">
              {r.ok ? '✓' : '✗'} {r.message}
            </div>
          {/if}

          <div class="mt-auto">
            <button
              data-testid="adapter-test-{adapter.name}"
              onclick={() => testAdapter(adapter.name)}
              disabled={testing[adapter.name] || !adapter.enabled}
              class="w-full flex items-center justify-center gap-2 px-3 py-2 text-xs rounded-lg
                     bg-gray-800 hover:bg-gray-700 text-gray-300 hover:text-white
                     disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            >
              {#if testing[adapter.name]}
                <RefreshCw size={12} class="animate-spin" /> Testing…
              {:else}
                <Play size={12} /> Test Connection
              {/if}
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>
