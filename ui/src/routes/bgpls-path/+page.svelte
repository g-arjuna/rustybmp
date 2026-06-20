<script lang="ts">
  import { api } from '$lib/api';
  import { onMount } from 'svelte';

  let from    = '';
  let to      = '';
  let result: any = null;
  let loading = false;
  let error   = '';

  // Pre-populate nodes from topology graph
  let knownNodes: string[] = [];

  onMount(async () => {
    try {
      const g = await api.bgplsGraph();
      knownNodes = g.nodes.map((n: any) => n.id);
    } catch { /* ignore */ }
  });

  async function findPath() {
    if (!from || !to) return;
    loading = true;
    error   = '';
    result  = null;
    try {
      result = await api.bgplsPath(from, to);
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }
</script>

<div class="p-6 max-w-3xl mx-auto space-y-6">
  <div>
    <h1 class="text-2xl font-bold text-white">BGP-LS Path Computation</h1>
    <p class="text-gray-400 text-sm mt-1">Dijkstra shortest IGP path between two routers using BGP-LS link metrics</p>
  </div>

  <div class="bg-gray-900/60 border border-gray-700 rounded-xl p-5 space-y-4">
    <div class="grid grid-cols-2 gap-4">
      <div class="space-y-1">
        <label class="text-xs text-gray-400">Source Router ID</label>
        <input
          bind:value={from}
          list="node-list"
          class="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-sm text-white font-mono"
          placeholder="10.0.0.1"
        />
      </div>
      <div class="space-y-1">
        <label class="text-xs text-gray-400">Destination Router ID</label>
        <input
          bind:value={to}
          list="node-list"
          class="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-sm text-white font-mono"
          placeholder="10.0.0.99"
        />
      </div>
    </div>

    <datalist id="node-list">
      {#each knownNodes as n}
        <option value={n} />
      {/each}
    </datalist>

    <button
      on:click={findPath}
      disabled={loading || !from || !to}
      class="px-5 py-2 bg-blue-600 hover:bg-blue-500 disabled:opacity-50 text-white rounded-lg text-sm font-medium transition-colors"
    >
      {loading ? 'Computing…' : '→ Find Path'}
    </button>
  </div>

  {#if error}
    <div class="text-red-400 text-sm bg-red-900/20 border border-red-800 rounded-lg p-3">{error}</div>
  {/if}

  {#if result}
    <div class="bg-gray-900/60 border border-gray-700 rounded-xl p-5 space-y-4">
      <div class="flex items-center gap-3">
        <span class="text-sm text-gray-400">Result:</span>
        {#if result.found}
          <span class="text-green-400 font-semibold text-sm">Path found ✓</span>
        {:else}
          <span class="text-red-400 font-semibold text-sm">No path found ✗</span>
        {/if}
      </div>

      {#if result.found && result.path?.length > 0}
        <!-- Visual path -->
        <div class="flex flex-wrap items-center gap-2 mt-2">
          {#each result.path as hop, i}
            <div class="bg-gray-800 border border-gray-700 rounded-lg px-3 py-1.5 font-mono text-sm text-white">
              {hop}
            </div>
            {#if i < result.path.length - 1}
              <span class="text-gray-500 text-lg">→</span>
            {/if}
          {/each}
        </div>
        <div class="text-xs text-gray-500">
          {result.path.length} hop{result.path.length !== 1 ? 's' : ''} from <span class="text-gray-300">{result.from}</span> to <span class="text-gray-300">{result.to}</span>
        </div>
      {/if}
    </div>
  {/if}

  <!-- Known nodes list -->
  {#if knownNodes.length > 0}
    <div class="bg-gray-900/40 border border-gray-800 rounded-xl p-4">
      <h2 class="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-3">
        Known BGP-LS Nodes ({knownNodes.length})
      </h2>
      <div class="flex flex-wrap gap-2">
        {#each knownNodes as n}
          <button
            on:click={() => { if (!from) from = n; else if (!to) to = n; }}
            class="text-xs font-mono bg-gray-800 hover:bg-gray-700 text-gray-300 px-2 py-1 rounded border border-gray-700 transition-colors"
          >{n}</button>
        {/each}
      </div>
    </div>
  {/if}
</div>
