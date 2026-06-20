<script lang="ts">
  import { onMount } from 'svelte';
  import { api, openEventStream, type RouteRow } from '$lib/api';
  import { RefreshCw, Search } from 'lucide-svelte';

  let routes   = $state<RouteRow[]>([]);
  let loading  = $state(true);
  let search   = $state('');
  let limit    = $state(200);
  let liveMode = $state(false);
  let es: EventSource | null = null;

  async function load() {
    loading = true;
    const params: Record<string, string> = { limit: String(limit) };
    if (search) params.prefix = search;
    routes  = await api.routes(params).catch(() => []);
    loading = false;
  }

  function toggleLive() {
    if (liveMode) {
      es?.close(); es = null; liveMode = false;
    } else {
      liveMode = true;
      es = openEventStream((type, data) => {
        if (type === 'route_change') {
          const row = data as RouteRow;
          routes = [row, ...routes.slice(0, limit - 1)];
        }
      });
    }
  }

  onMount(load);

  import { onDestroy } from 'svelte';
  onDestroy(() => es?.close());

  const filtered = $derived(
    search ? routes.filter(r => r.prefix.includes(search)) : routes
  );
</script>

<div class="p-6 space-y-5">
  <div class="flex items-center justify-between flex-wrap gap-3">
    <h1 class="text-2xl font-bold text-gray-100">Prefixes</h1>
    <div class="flex items-center gap-2">
      <div class="relative">
        <Search size={13} class="absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-500" />
        <input
          bind:value={search}
          onchange={load}
          placeholder="Search prefix…"
          class="bg-gray-900 border border-gray-700 rounded-lg pl-7 pr-3 py-1.5 text-sm text-gray-200
                 placeholder-gray-600 focus:outline-none focus:border-emerald-500 w-48"
        />
      </div>
      <button
        onclick={toggleLive}
        class="px-3 py-1.5 rounded-lg text-xs font-medium transition-colors
               {liveMode
                 ? 'bg-emerald-500/20 text-emerald-400 border border-emerald-500/40'
                 : 'bg-gray-800 text-gray-400 border border-gray-700 hover:text-gray-100'}"
      >
        {liveMode ? '⬤ Live' : 'Live'}
      </button>
      <button
        onclick={load}
        class="p-2 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-gray-100"
      >
        <RefreshCw size={14} />
      </button>
    </div>
  </div>

  {#if loading}
    <p class="text-gray-500 text-sm">Loading…</p>
  {:else}
    <div class="bg-gray-900 rounded-xl border border-gray-800 overflow-hidden overflow-x-auto">
      <table class="w-full text-sm min-w-[800px]">
        <thead>
          <tr class="border-b border-gray-800 text-gray-500 text-xs uppercase tracking-wider">
            <th class="px-4 py-3 text-left">Prefix</th>
            <th class="px-4 py-3 text-left">Peer</th>
            <th class="px-4 py-3 text-left">Action</th>
            <th class="px-4 py-3 text-left">AS Path</th>
            <th class="px-4 py-3 text-left">Next Hop</th>
            <th class="px-4 py-3 text-right">LP</th>
            <th class="px-4 py-3 text-left">Communities</th>
            <th class="px-4 py-3 text-right">Time</th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as r}
            <tr class="border-b border-gray-800/50 hover:bg-gray-800/30 transition-colors">
              <td class="px-4 py-2.5 font-mono text-emerald-300 text-xs">{r.prefix}</td>
              <td class="px-4 py-2.5 font-mono text-gray-400 text-xs">{r.peer_addr}</td>
              <td class="px-4 py-2.5">
                <span class="px-1.5 py-0.5 rounded text-xs font-medium
                  {r.action === 'announce' ? 'bg-emerald-500/15 text-emerald-400' : 'bg-amber-500/15 text-amber-400'}">
                  {r.action}
                </span>
              </td>
              <td class="px-4 py-2.5 font-mono text-gray-400 text-xs max-w-[200px] truncate">{r.as_path ?? '—'}</td>
              <td class="px-4 py-2.5 font-mono text-gray-400 text-xs">{r.next_hop ?? '—'}</td>
              <td class="px-4 py-2.5 text-right text-gray-400 text-xs">{r.local_pref ?? '—'}</td>
              <td class="px-4 py-2.5 text-gray-500 text-xs max-w-[160px] truncate">{r.communities ?? '—'}</td>
              <td class="px-4 py-2.5 text-right text-gray-600 text-xs">{new Date(r.occurred_at).toLocaleTimeString()}</td>
            </tr>
          {:else}
            <tr>
              <td colspan="8" class="px-4 py-8 text-center text-gray-600 italic">No routes found</td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="text-xs text-gray-600">{filtered.length} routes shown</p>
  {/if}
</div>
