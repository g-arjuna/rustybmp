<script lang="ts">
  import { onMount } from 'svelte';
  import { Radio, RefreshCw, Search } from 'lucide-svelte';
  import AsnSankey from '$lib/AsnSankey.svelte';
  import MetricCard from '$lib/MetricCard.svelte';

  type AsPathRow = {
    prefix:      string;
    peer_addr:   string;
    as_path:     string | null;
    as_path_len: number | null;
    origin:      string | null;
    next_hop:    string | null;
    occurred_at: string;
  };

  let rows: AsPathRow[] = $state([]);
  let sankeyNodes: any[] = $state([]);
  let sankeyLinks: any[] = $state([]);
  let loading  = $state(true);
  let error    = $state('');
  let search   = $state('');
  let limit    = $state(200);
  let showSankey = $state(true);

  async function load() {
    loading = true; error = '';
    try {
      const params = new URLSearchParams({ limit: String(limit), action: 'announce' });
      if (search) params.set('prefix', search);
      const [routeRes, graphRes] = await Promise.all([
        fetch(`/api/routes?${params}`).then(r => r.json()),
        fetch(`/api/aspath/graph?limit=300`).then(r => r.json()),
      ]);
      rows        = (routeRes as any).routes ?? [];
      sankeyNodes = (graphRes as any).nodes  ?? [];
      sankeyLinks = (graphRes as any).links  ?? [];
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }
  onMount(load);

  // AS-path length distribution
  const distribution = $derived(rows.reduce((acc: Record<number,number>, r) => {
    const len = r.as_path_len ?? 0;
    acc[len] = (acc[len] ?? 0) + 1;
    return acc;
  }, {} as Record<number, number>));
  const maxLenCount = $derived(Math.max(1, ...Object.values(distribution)));
  const sortedLens  = $derived(Object.keys(distribution).map(Number).sort((a, b) => a - b));

  const filtered = $derived(
    search ? rows.filter(r => r.prefix.includes(search) || (r.as_path ?? '').includes(search)) : rows
  );

  function fmt(dt: string) { return new Date(dt).toLocaleString(); }
</script>

<svelte:head><title>AS Path Explorer — RustyBMP</title></svelte:head>

<div class="p-6 space-y-6 max-w-7xl mx-auto">
  <div class="flex items-center justify-between flex-wrap gap-3">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <Radio size={22} class="text-indigo-400" /> AS Path Explorer
    </h1>
    <div class="flex items-center gap-2">
      <div class="relative">
        <Search size={13} class="absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-500" />
        <input bind:value={search} on:change={load} placeholder="prefix or AS…"
          class="bg-gray-900 border border-gray-700 rounded-lg pl-7 pr-3 py-1.5 text-sm text-gray-200
                 placeholder-gray-600 focus:outline-none focus:border-indigo-500 w-48" />
      </div>
      <button on:click={load}
        class="p-2 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-gray-100">
        <RefreshCw size={14} />
      </button>
    </div>
  </div>

  {#if error}
    <div class="bg-red-900/30 border border-red-700 text-red-300 rounded p-4 text-sm">{error}</div>
  {/if}

  {#if !loading}
    <div class="grid grid-cols-3 gap-4">
      <MetricCard label="Routes Loaded"    value={rows.length}            color="blue" />
      <MetricCard label="Unique ASN Pairs" value={sankeyLinks.length}     color="purple" />
      <MetricCard label="AS Nodes"         value={sankeyNodes.length}     color="green" />
    </div>
  {/if}

  {#if !loading && sortedLens.length > 0}
    <div class="grid grid-cols-1 lg:grid-cols-2 gap-4">
      <!-- AS path length distribution chart -->
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-5">
        <h2 class="text-sm font-semibold text-gray-300 mb-4">AS Path Length Distribution</h2>
        <div class="flex items-end gap-1 h-24">
          {#each sortedLens as len}
            {@const count = distribution[len]}
            {@const h = Math.round(count / maxLenCount * 88)}
            <div class="flex flex-col items-center gap-0.5 flex-1" title="len={len}: {count} routes">
              <div class="w-full bg-indigo-500 rounded-t" style="height:{h}px"></div>
              <span class="text-xs text-gray-600">{len}</span>
            </div>
          {/each}
        </div>
        <div class="text-xs text-gray-600 mt-1">hop count →</div>
      </div>

      <!-- AS Flow (Sankey) -->
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-5">
        <div class="flex items-center justify-between mb-3">
          <h2 class="text-sm font-semibold text-gray-300">AS Path Flow</h2>
          <button on:click={() => showSankey = !showSankey}
            class="text-xs text-gray-500 hover:text-gray-300">
            {showSankey ? 'Hide' : 'Show'}
          </button>
        </div>
        {#if showSankey}
          <AsnSankey nodes={sankeyNodes} links={sankeyLinks} height={220} />
        {/if}
      </div>
    </div>
  {/if}

  {#if loading}
    <div class="text-gray-500 text-sm animate-pulse">Loading routes…</div>
  {:else}
    <div class="bg-gray-900 border border-gray-800 rounded-lg overflow-hidden overflow-x-auto">
      <table class="w-full text-xs text-left min-w-[700px]">
        <thead>
          <tr class="text-gray-500 border-b border-gray-800 uppercase tracking-wider text-xs">
            <th class="px-4 py-3">Prefix</th>
            <th class="px-4 py-3">Peer</th>
            <th class="px-4 py-3">AS Path</th>
            <th class="px-4 py-3 text-right">Hops</th>
            <th class="px-4 py-3">Origin</th>
            <th class="px-4 py-3">Next Hop</th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as r}
            <tr class="border-b border-gray-800/50 hover:bg-gray-800/30">
              <td class="px-4 py-2 font-mono">
                <a href="/prefix/{encodeURIComponent(r.prefix)}"
                   class="text-emerald-400 hover:underline">{r.prefix}</a>
              </td>
              <td class="px-4 py-2 font-mono text-blue-400">{r.peer_addr}</td>
              <td class="px-4 py-2 font-mono text-gray-300 max-w-xs truncate" title={r.as_path ?? ''}>
                {#if r.as_path}
                  {#each r.as_path.split(' ') as asn, i}
                    <span>{#if i > 0}<span class="text-gray-600 mx-0.5">›</span>{/if}<span class="text-indigo-300">{asn}</span></span>
                  {/each}
                {:else}—{/if}
              </td>
              <td class="px-4 py-2 text-right text-gray-400">{r.as_path_len ?? '—'}</td>
              <td class="px-4 py-2 text-gray-500">{r.origin ?? '—'}</td>
              <td class="px-4 py-2 font-mono text-gray-500">{r.next_hop ?? '—'}</td>
            </tr>
          {:else}
            <tr><td colspan="6" class="px-4 py-8 text-center text-gray-600 italic">No routes</td></tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="text-xs text-gray-600">{filtered.length} routes</div>
  {/if}
</div>
