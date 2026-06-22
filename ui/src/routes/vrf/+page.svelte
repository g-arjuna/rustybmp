<script lang="ts">
  import { onMount } from 'svelte';
  import { RefreshCw, Layers, Route } from 'lucide-svelte';

  interface VrfSummary {
    rd:           string;
    vrf_name:     string | null;
    route_count:  number;
    peer_count:   number;
    afi:          string;
  }

  let vrfs    = $state<VrfSummary[]>([]);
  let loading = $state(true);
  let selected = $state('');

  interface RouteRow {
    occurred_at:  string;
    peer_addr:    string;
    prefix:       string;
    action:       string;
    as_path:      string | null;
    communities:  string | null;
    next_hop:     string | null;
    local_pref:   number | null;
  }

  let routes   = $state<RouteRow[]>([]);
  let routeLoading = $state(false);

  async function load() {
    loading = true;
    try {
      const res = await fetch('/api/vrf/list');
      const j   = await res.json() as { vrfs: VrfSummary[] };
      vrfs = j.vrfs ?? [];
      if (vrfs.length > 0 && !selected) selected = vrfs[0].rd;
    } finally {
      loading = false;
    }
  }

  async function loadRoutes() {
    if (!selected) return;
    routeLoading = true;
    try {
      const res = await fetch(`/api/vrf/${encodeURIComponent(selected)}/routes`);
      const j   = await res.json() as { routes: RouteRow[] };
      routes = j.routes ?? [];
    } finally {
      routeLoading = false;
    }
  }

  $effect(() => {
    if (selected) loadRoutes();
  });

  onMount(load);

  const selectedVrf = $derived(vrfs.find(v => v.rd === selected));
</script>

<svelte:head><title>VRF Explorer — RustyBMP</title></svelte:head>

<div data-testid="page-vrf" class="p-6 space-y-6">
  <div class="flex items-center justify-between flex-wrap gap-3">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <Layers size={22} class="text-teal-400" /> VRF Explorer
    </h1>
    <button
      data-testid="vrf-refresh"
      onclick={load}
      class="p-2 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-gray-100 transition-colors"
      title="Refresh"
    >
      <RefreshCw size={15} />
    </button>
  </div>

  <p class="text-sm text-gray-500">
    Browse routes segmented by VPN Route Distinguisher (L3VPN RFC 4364 / EVPN RFC 7432).
    Switch VRFs using the selector below.
  </p>

  {#if loading}
    <p class="text-gray-500 text-sm animate-pulse">Loading VRFs…</p>
  {:else if vrfs.length === 0}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-10 text-center space-y-3">
      <Layers size={36} class="text-gray-700 mx-auto" />
      <p class="text-gray-500 text-sm">No VRF/VPN routes detected.</p>
      <p class="text-xs text-gray-600">
        L3VPN (AFI=1/2 SAFI=128) or EVPN (AFI=25 SAFI=70) sessions are required.
      </p>
    </div>
  {:else}
    <!-- VRF selector row -->
    <div class="flex items-start gap-4 flex-wrap">
      <div class="space-y-1.5">
        <label class="text-xs text-gray-500 block">Select VRF / Route Distinguisher</label>
        <select
          data-testid="vrf-selector"
          bind:value={selected}
          class="bg-gray-900 border border-gray-700 rounded-lg px-3 py-2 text-sm text-gray-200
                 focus:outline-none focus:border-teal-500 font-mono min-w-60"
        >
          {#each vrfs as vrf}
            <option value={vrf.rd}>
              {vrf.vrf_name ?? vrf.rd} — {vrf.route_count.toLocaleString()} routes ({vrf.afi})
            </option>
          {/each}
        </select>
      </div>

      {#if selectedVrf}
        <div class="flex gap-4 pt-5">
          <div class="text-center">
            <div class="text-xl font-bold text-teal-400">{selectedVrf.route_count.toLocaleString()}</div>
            <div class="text-xs text-gray-500">routes</div>
          </div>
          <div class="text-center">
            <div class="text-xl font-bold text-gray-300">{selectedVrf.peer_count}</div>
            <div class="text-xs text-gray-500">peers</div>
          </div>
          <div class="text-center">
            <div class="text-sm font-mono text-gray-300 pt-1">{selectedVrf.afi}</div>
            <div class="text-xs text-gray-500">AFI</div>
          </div>
        </div>
      {/if}
    </div>

    <!-- VRF summary cards -->
    <div class="grid grid-cols-2 md:grid-cols-4 gap-3">
      {#each vrfs.slice(0, 8) as vrf}
        <button
          data-testid="vrf-card-{vrf.rd}"
          onclick={() => { selected = vrf.rd; }}
          class="text-left bg-gray-900 border rounded-xl p-3 transition-colors
                 {selected === vrf.rd
                   ? 'border-teal-600 bg-teal-900/15'
                   : 'border-gray-800 hover:border-gray-700'}"
        >
          <div class="text-xs font-mono text-teal-300 truncate">{vrf.vrf_name ?? vrf.rd}</div>
          <div class="text-xs text-gray-500 font-mono mt-0.5">{vrf.rd}</div>
          <div class="mt-2 text-sm font-semibold text-gray-200">{vrf.route_count.toLocaleString()}</div>
          <div class="text-xs text-gray-600">{vrf.afi}</div>
        </button>
      {/each}
      {#if vrfs.length > 8}
        <div class="flex items-center justify-center text-xs text-gray-600 bg-gray-900/40 border border-gray-800 rounded-xl p-3">
          +{vrfs.length - 8} more VRFs
        </div>
      {/if}
    </div>

    <!-- Route table for selected VRF -->
    {#if selected}
      <div class="space-y-2">
        <h2 class="text-sm font-semibold text-gray-300 flex items-center gap-2">
          <Route size={14} class="text-teal-400" />
          Routes in <span class="font-mono text-teal-300">{selectedVrf?.vrf_name ?? selected}</span>
        </h2>

        {#if routeLoading}
          <p class="text-gray-500 text-sm animate-pulse">Loading routes…</p>
        {:else}
          <div class="bg-gray-900 border border-gray-800 rounded-xl overflow-auto">
            <table data-testid="vrf-routes-table" class="w-full text-sm min-w-max">
              <thead>
                <tr class="border-b border-gray-800 text-gray-500 text-xs uppercase tracking-wider">
                  <th class="px-4 py-3 text-left">Prefix</th>
                  <th class="px-4 py-3 text-left">Peer</th>
                  <th class="px-4 py-3 text-left">Action</th>
                  <th class="px-4 py-3 text-left">Next Hop</th>
                  <th class="px-4 py-3 text-right">LP</th>
                  <th class="px-4 py-3 text-left">AS Path</th>
                  <th class="px-4 py-3 text-left">Communities</th>
                  <th class="px-4 py-3 text-right">Received</th>
                </tr>
              </thead>
              <tbody>
                {#each routes as row}
                  <tr class="border-b border-gray-800/50 hover:bg-gray-800/30 transition-colors">
                    <td class="px-4 py-3 font-mono text-blue-300 text-xs">{row.prefix}</td>
                    <td class="px-4 py-3 font-mono text-gray-400 text-xs">{row.peer_addr}</td>
                    <td class="px-4 py-3">
                      <span class="px-2 py-0.5 rounded text-xs font-medium
                        {row.action === 'announce' ? 'bg-emerald-500/15 text-emerald-400' : 'bg-red-500/15 text-red-400'}">
                        {row.action}
                      </span>
                    </td>
                    <td class="px-4 py-3 font-mono text-gray-400 text-xs">{row.next_hop ?? '—'}</td>
                    <td class="px-4 py-3 text-right text-gray-400 text-xs font-mono">{row.local_pref ?? '—'}</td>
                    <td class="px-4 py-3 font-mono text-gray-400 text-xs max-w-xs truncate" title={row.as_path ?? ''}>
                      {row.as_path ?? '—'}
                    </td>
                    <td class="px-4 py-3 font-mono text-yellow-400/80 text-xs max-w-xs truncate" title={row.communities ?? ''}>
                      {row.communities ?? '—'}
                    </td>
                    <td class="px-4 py-3 text-right text-gray-500 text-xs">
                      {new Date(row.occurred_at).toLocaleTimeString()}
                    </td>
                  </tr>
                {:else}
                  <tr>
                    <td colspan="8" class="px-4 py-8 text-center text-gray-600 italic">No routes in this VRF</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
          <p class="text-xs text-gray-600">{routes.length} routes</p>
        {/if}
      </div>
    {/if}
  {/if}
</div>
