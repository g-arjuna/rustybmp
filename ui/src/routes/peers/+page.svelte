<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type PeerSummary } from '$lib/api';
  import { RefreshCw } from 'lucide-svelte';

  let peers    = $state<PeerSummary[]>([]);
  let loading  = $state(true);
  let search   = $state('');

  async function load() {
    loading = true;
    peers = await api.peers().catch(() => []);
    loading = false;
  }

  onMount(load);

  const filtered = $derived(
    search
      ? peers.filter(p =>
          p.peer_addr.includes(search) ||
          String(p.peer_as).includes(search)
        )
      : peers
  );
</script>

<div class="p-6 space-y-5">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100">Peers</h1>
    <div class="flex items-center gap-3">
      <input
        bind:value={search}
        placeholder="Filter peers…"
        class="bg-gray-900 border border-gray-700 rounded-lg px-3 py-1.5 text-sm text-gray-200
               placeholder-gray-600 focus:outline-none focus:border-emerald-500 w-52"
      />
      <button
        onclick={load}
        class="p-2 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-gray-100 transition-colors"
        title="Refresh"
      >
        <RefreshCw size={15} />
      </button>
    </div>
  </div>

  {#if loading}
    <p class="text-gray-500 text-sm">Loading…</p>
  {:else}
    <div class="bg-gray-900 rounded-xl border border-gray-800 overflow-hidden">
      <table class="w-full text-sm">
        <thead>
          <tr class="border-b border-gray-800 text-gray-500 text-xs uppercase tracking-wider">
            <th class="px-4 py-3 text-left">Peer</th>
            <th class="px-4 py-3 text-left">AS</th>
            <th class="px-4 py-3 text-left">RIB</th>
            <th class="px-4 py-3 text-left">State</th>
            <th class="px-4 py-3 text-right">Prefixes</th>
            <th class="px-4 py-3 text-right">Hold</th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as peer}
            <tr class="border-b border-gray-800/50 hover:bg-gray-800/30 transition-colors">
              <td class="px-4 py-3 font-mono text-gray-200">{peer.peer_addr}</td>
              <td class="px-4 py-3 text-gray-400">AS{peer.peer_as}</td>
              <td class="px-4 py-3 text-gray-400">{peer.rib_type}</td>
              <td class="px-4 py-3">
                <span class="px-2 py-0.5 rounded text-xs font-medium
                  {peer.state === 'up' ? 'bg-emerald-500/15 text-emerald-400' : 'bg-red-500/15 text-red-400'}">
                  {peer.state}
                </span>
              </td>
              <td class="px-4 py-3 text-right text-gray-400">{peer.prefix_count.toLocaleString()}</td>
              <td class="px-4 py-3 text-right text-gray-500">{peer.hold_time ?? '—'}s</td>
            </tr>
          {:else}
            <tr>
              <td colspan="6" class="px-4 py-8 text-center text-gray-600 italic">No peers found</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="text-xs text-gray-600">{filtered.length} of {peers.length} peers</p>
  {/if}
</div>
