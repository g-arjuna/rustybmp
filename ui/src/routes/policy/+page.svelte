<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { GitBranch, RefreshCw, Search } from 'lucide-svelte';

  type ByRibType  = { rib_type: string; prefix_count: number };
  type PolicyData = { peer_addr: string; by_rib_type: ByRibType[] };

  let peerAddr   = $state('');
  let data:  PolicyData | null = $state(null);
  let peers: string[]          = $state([]);
  let loading    = $state(false);
  let error      = $state('');

  async function loadPeers() {
    try {
      const res = await fetch('/api/peers');
      const j   = await res.json() as { peers: { addr: string }[] };
      peers = j.peers?.map((p: { addr: string }) => p.addr) ?? [];
    } catch {}
  }

  async function loadPolicy() {
    if (!peerAddr) return;
    loading = true; error = '';
    try {
      data = await api.policyDelta(peerAddr) as PolicyData;
    } catch (e) {
      error = String(e);
      data  = null;
    } finally {
      loading = false;
    }
  }

  onMount(loadPeers);

  const totalPrefixes = $derived((data as PolicyData | null)?.by_rib_type?.reduce((s: number, r: ByRibType) => s + r.prefix_count, 0) ?? 0);

  const RIB_COLORS: Record<string, string> = {
    'pre-policy':  'bg-blue-500',
    'post-policy': 'bg-emerald-500',
    'loc-rib':     'bg-purple-500',
  };
</script>

<svelte:head><title>Policy Analysis — RustyBMP</title></svelte:head>

<div class="p-6 space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <GitBranch size={22} class="text-blue-400" /> Policy Analysis
    </h1>
  </div>

  <!-- Peer selector -->
  <div class="bg-gray-900 border border-gray-800 rounded-lg p-5 space-y-3">
    <label class="block text-sm text-gray-400">Select Peer</label>
    <div class="flex gap-2">
      {#if peers.length > 0}
        <select
          bind:value={peerAddr}
          class="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-blue-500"
        >
          <option value="">— choose a peer —</option>
          {#each peers as p}<option value={p}>{p}</option>{/each}
        </select>
      {:else}
        <input
          bind:value={peerAddr}
          placeholder="192.0.2.1"
          class="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-2 text-sm text-gray-200
                 placeholder-gray-600 focus:outline-none focus:border-blue-500 font-mono"
        />
      {/if}
      <button
        disabled={!peerAddr}
        on:click={loadPolicy}
        class="px-4 py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-40 text-white text-sm rounded"
      >
        Analyse
      </button>
    </div>
  </div>

  {#if error}
    <div class="bg-red-900/30 border border-red-700 text-red-300 rounded p-4 text-sm">{error}</div>
  {/if}

  {#if loading}
    <div class="text-gray-500 text-sm animate-pulse">Analysing policy…</div>
  {:else if data}
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-5 space-y-4">
      <h2 class="text-sm font-semibold text-gray-300">
        RIB breakdown for <span class="font-mono text-blue-400">{data.peer_addr}</span>
      </h2>

      <!-- Visual bars -->
      <div class="space-y-3">
        {#each data.by_rib_type as row}
          {@const pct = totalPrefixes > 0 ? Math.round(row.prefix_count / totalPrefixes * 100) : 0}
          {@const color = RIB_COLORS[row.rib_type] ?? 'bg-gray-500'}
          <div>
            <div class="flex justify-between text-xs text-gray-400 mb-1">
              <span class="font-mono">{row.rib_type}</span>
              <span>{row.prefix_count.toLocaleString()} prefixes ({pct}%)</span>
            </div>
            <div class="h-3 bg-gray-800 rounded-full overflow-hidden">
              <div class="h-3 {color} rounded-full transition-all" style="width:{pct}%"></div>
            </div>
          </div>
        {/each}
      </div>

      <!-- Policy delta hint -->
      {#if data.by_rib_type.length >= 2}
        {@const pre  = data.by_rib_type.find(r => r.rib_type === 'pre-policy')?.prefix_count ?? 0}
        {@const post = data.by_rib_type.find(r => r.rib_type === 'post-policy')?.prefix_count ?? 0}
        {@const delta = pre - post}
        {#if delta > 0}
          <div class="mt-3 p-3 bg-yellow-900/20 border border-yellow-700/40 rounded text-xs text-yellow-300">
            ⚠ {delta} prefixes filtered between pre-policy and post-policy ({Math.round(delta/pre*100)}% policy drop rate)
          </div>
        {:else if delta === 0}
          <div class="mt-3 p-3 bg-emerald-900/20 border border-emerald-700/40 rounded text-xs text-emerald-300">
            ✓ No route filtering detected between pre- and post-policy RIB.
          </div>
        {/if}
      {/if}
    </div>
  {/if}
</div>
