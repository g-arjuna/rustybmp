<script lang="ts">
  import { onMount } from 'svelte';
  import { Zap, RefreshCw, AlertTriangle, Shield } from 'lucide-svelte';

  interface FlowspecRule {
    id:              string;
    speaker_addr:    string;
    peer_addr:       string;
    source_asn:      number | null;
    dst_prefix:      string | null;
    src_prefix:      string | null;
    proto:           string | null;
    dst_port:        string | null;
    src_port:        string | null;
    action:          string;
    rate_bps:        number | null;
    community:       string | null;
    received_at:     string;
    large_prefix:    boolean;
  }

  let rules    = $state<FlowspecRule[]>([]);
  let loading  = $state(true);
  let speaker  = $state('');
  let speakers = $state<string[]>([]);

  async function load() {
    loading = true;
    try {
      const [rr, sr] = await Promise.allSettled([
        fetch('/api/flowspec/rules' + (speaker ? `?speaker=${encodeURIComponent(speaker)}` : ''))
          .then(r => r.json() as Promise<{ rules: FlowspecRule[] }>),
        fetch('/api/speakers').then(r => r.json() as Promise<{ speakers: { addr: string }[] }>),
      ]);
      rules    = rr.status === 'fulfilled' ? rr.value.rules ?? [] : [];
      speakers = sr.status === 'fulfilled' ? sr.value.speakers?.map((s: { addr: string }) => s.addr) ?? [] : [];
    } finally {
      loading = false;
    }
  }

  onMount(load);

  const largePrefixRules = $derived(rules.filter(r => r.large_prefix));

  function actionColor(action: string) {
    if (action.includes('rate=0') || action === 'drop')    return 'bg-red-500/15 text-red-400';
    if (action.includes('redirect'))                        return 'bg-yellow-500/15 text-yellow-400';
    if (action.includes('rate'))                            return 'bg-orange-500/15 text-orange-400';
    return 'bg-gray-500/15 text-gray-400';
  }

  function actionLabel(r: FlowspecRule) {
    if (r.rate_bps === 0)  return 'DROP';
    if (r.rate_bps !== null) return `RATE-LIMIT ${(r.rate_bps / 1_000_000).toFixed(1)} Mbps`;
    return r.action || 'unknown';
  }
</script>

<svelte:head><title>FlowSpec Rules — RustyBMP</title></svelte:head>

<div data-testid="page-flowspec" class="p-6 space-y-6">
  <div class="flex items-center justify-between flex-wrap gap-3">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <Zap size={22} class="text-orange-400" /> FlowSpec Rules
    </h1>
    <div class="flex items-center gap-2">
      <select
        data-testid="flowspec-speaker-filter"
        bind:value={speaker}
        onchange={load}
        class="bg-gray-900 border border-gray-700 rounded-lg px-3 py-1.5 text-sm text-gray-200
               focus:outline-none focus:border-emerald-500 font-mono"
      >
        <option value="">All speakers</option>
        {#each speakers as s}<option value={s}>{s}</option>{/each}
      </select>
      <button
        data-testid="flowspec-refresh"
        onclick={load}
        class="p-2 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-gray-100 transition-colors"
      >
        <RefreshCw size={15} />
      </button>
    </div>
  </div>

  <!-- Large-prefix alert -->
  {#if largePrefixRules.length > 0}
    <div data-testid="flowspec-large-prefix-alert"
         class="bg-orange-900/20 border border-orange-700/40 rounded-xl p-4 flex items-start gap-3">
      <AlertTriangle size={16} class="text-orange-400 mt-0.5 flex-shrink-0" />
      <div>
        <p class="text-sm text-orange-300 font-medium">
          {largePrefixRules.length} rule{largePrefixRules.length > 1 ? 's' : ''} cover a large prefix — possible DDoS mitigation in progress
        </p>
        <ul class="mt-1 space-y-0.5">
          {#each largePrefixRules as r}
            <li class="text-xs text-orange-400 font-mono">
              {r.dst_prefix ?? r.src_prefix ?? '?'} via {r.speaker_addr}
            </li>
          {/each}
        </ul>
      </div>
    </div>
  {/if}

  {#if loading}
    <p class="text-gray-500 text-sm animate-pulse">Loading FlowSpec rules…</p>
  {:else if rules.length === 0}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-10 text-center space-y-3">
      <Shield size={36} class="text-gray-700 mx-auto" />
      <p class="text-gray-500 text-sm">No active FlowSpec rules.</p>
      <p class="text-xs text-gray-600">FlowSpec rules (RFC 5575) are propagated via BGP SAFI 133/134.</p>
    </div>
  {:else}
    <div class="bg-gray-900 border border-gray-800 rounded-xl overflow-auto">
      <table data-testid="flowspec-table" class="w-full text-sm min-w-max">
        <thead>
          <tr class="border-b border-gray-800 text-gray-500 text-xs uppercase tracking-wider">
            <th class="px-4 py-3 text-left">Speaker</th>
            <th class="px-4 py-3 text-left">Peer</th>
            <th class="px-4 py-3 text-left">Dst Prefix</th>
            <th class="px-4 py-3 text-left">Src Prefix</th>
            <th class="px-4 py-3 text-left">Proto</th>
            <th class="px-4 py-3 text-left">Dst Port</th>
            <th class="px-4 py-3 text-left">Action</th>
            <th class="px-4 py-3 text-left">Community</th>
            <th class="px-4 py-3 text-right">Received</th>
          </tr>
        </thead>
        <tbody>
          {#each rules as rule}
            <tr data-testid="flowspec-row-{rule.id}"
                class="border-b border-gray-800/50 hover:bg-gray-800/30 transition-colors
                       {rule.large_prefix ? 'bg-orange-900/10' : ''}">
              <td class="px-4 py-3 font-mono text-gray-300 text-xs">{rule.speaker_addr}</td>
              <td class="px-4 py-3 font-mono text-gray-400 text-xs">{rule.peer_addr}</td>
              <td class="px-4 py-3 font-mono text-xs">
                {#if rule.dst_prefix}
                  <span class="text-blue-300">{rule.dst_prefix}</span>
                  {#if rule.large_prefix}
                    <AlertTriangle size={11} class="inline text-orange-400 ml-1" />
                  {/if}
                {:else}
                  <span class="text-gray-600">any</span>
                {/if}
              </td>
              <td class="px-4 py-3 font-mono text-gray-400 text-xs">{rule.src_prefix ?? 'any'}</td>
              <td class="px-4 py-3 font-mono text-gray-400 text-xs">{rule.proto ?? 'any'}</td>
              <td class="px-4 py-3 font-mono text-gray-400 text-xs">{rule.dst_port ?? 'any'}</td>
              <td class="px-4 py-3">
                <span class="px-2 py-0.5 rounded text-xs font-medium {actionColor(rule.action)}">
                  {actionLabel(rule)}
                </span>
              </td>
              <td class="px-4 py-3 font-mono text-gray-500 text-xs">{rule.community ?? '—'}</td>
              <td class="px-4 py-3 text-right text-gray-500 text-xs">
                {new Date(rule.received_at).toLocaleTimeString()}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="text-xs text-gray-600">{rules.length} active rule{rules.length === 1 ? '' : 's'}</p>
  {/if}
</div>
