<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';

  // ── Path status colour / icon map (RFC 9069 bitmap) ──────────────────────
  const STATUS_DEFS: { mask: number; icon: string; label: string; cls: string }[] = [
    { mask: 0x0002, icon: '★', label: 'Best',              cls: 'bg-emerald-500 text-white' },
    { mask: 0x0008, icon: '≡', label: 'Primary/ECMP',      cls: 'bg-emerald-700 text-white' },
    { mask: 0x0010, icon: '↻', label: 'Backup',            cls: 'bg-sky-500 text-white' },
    { mask: 0x0040, icon: '⊕', label: 'Best-external',     cls: 'bg-cyan-500 text-white' },
    { mask: 0x0004, icon: '✗', label: 'Nonselected',       cls: 'bg-amber-500 text-black' },
    { mask: 0x0100, icon: '⊘', label: 'Filtered-inbound',  cls: 'bg-red-500 text-white' },
    { mask: 0x0001, icon: '⊘', label: 'Invalid',           cls: 'bg-red-600 text-white' },
    { mask: 0x0400, icon: '💤', label: 'Stale',             cls: 'bg-gray-600 text-white' },
    { mask: 0x0800, icon: '⚡', label: 'Suppressed/RFD',    cls: 'bg-purple-600 text-white' },
  ];

  function statusInfo(bits: number) {
    for (const d of STATUS_DEFS) {
      if (bits & d.mask) return d;
    }
    return { icon: '—', label: 'Unknown', cls: 'bg-gray-800 text-gray-400' };
  }

  // ── State ─────────────────────────────────────────────────────────────────
  let matrix: any[] = [];
  let loading  = true;
  let error    = '';
  let afi      = 'ipv4';
  let minPaths = 1;
  let search   = '';

  async function load() {
    loading = true;
    error   = '';
    try {
      const r = await api.pathStatusMatrix({ afi, min_active_paths: String(minPaths), limit: '1000' });
      matrix = (r as any).rows ?? [];
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  }

  onMount(load);

  // ── Derived: pivot to prefix → { peer → row } ────────────────────────────
  $: allPeers   = [...new Set(matrix.map((r: any) => r.peer_addr as string))].sort();
  $: byPrefix   = (() => {
    const m = new Map<string, Map<string, any>>();
    for (const row of matrix) {
      if (!m.has(row.prefix)) m.set(row.prefix, new Map());
      m.get(row.prefix)!.set(row.peer_addr, row);
    }
    return m;
  })();

  $: filteredPrefixes = [...byPrefix.keys()].filter(p =>
    !search || p.toLowerCase().includes(search.toLowerCase())
  );

  // Active path count = paths that are not nonselected / filtered / stale
  function activePaths(peerMap: Map<string, any>): number {
    let n = 0;
    for (const row of peerMap.values()) {
      const bits = row.path_status as number;
      if (bits & (0x0002 | 0x0008 | 0x0010 | 0x0040)) n++;
    }
    return n;
  }

  function healthClass(ap: number): string {
    if (ap === 0) return 'bg-red-900/40 border-red-700';
    if (ap < 2)   return 'bg-amber-900/40 border-amber-700';
    return '';
  }
</script>

<div data-testid="page-path-status" class="p-6 max-w-full space-y-5">
  <div>
    <h1 class="text-2xl font-bold text-white">Path Status Matrix</h1>
    <p class="text-gray-400 text-sm mt-1">
      RFC 9069 Path Status TLV — per-prefix × per-peer decision process view
    </p>
  </div>

  <!-- Controls -->
  <div class="flex flex-wrap gap-3 items-center">
    <select data-testid="path-status-afi-filter" bind:value={afi} on:change={load}
      class="bg-gray-800 border border-gray-700 rounded-lg px-3 py-1.5 text-sm text-white">
      <option value="ipv4">IPv4</option>
      <option value="ipv6">IPv6</option>
    </select>

    <label class="flex items-center gap-2 text-sm text-gray-300">
      <span>Min active paths</span>
      <select bind:value={minPaths} on:change={load}
        class="bg-gray-800 border border-gray-700 rounded-lg px-2 py-1 text-sm text-white">
        <option value={1}>≥ 1</option>
        <option value={2}>≥ 2</option>
        <option value={3}>≥ 3</option>
        <option value={0}>Any</option>
      </select>
    </label>

    <input data-testid="path-status-search" bind:value={search} placeholder="Filter prefix…"
      class="bg-gray-800 border border-gray-700 rounded-lg px-3 py-1.5 text-sm text-white w-48" />

    <button data-testid="path-status-refresh" on:click={load}
      class="bg-blue-700 hover:bg-blue-600 text-white text-sm px-4 py-1.5 rounded-lg">
      Refresh
    </button>
  </div>

  <!-- Legend -->
  <div class="flex flex-wrap gap-2">
    {#each STATUS_DEFS as d}
      <span class="flex items-center gap-1 px-2 py-0.5 rounded text-xs {d.cls}">
        {d.icon} {d.label}
      </span>
    {/each}
  </div>

  {#if loading}
    <div class="h-40 bg-gray-800/50 rounded-xl animate-pulse" />
  {:else if error}
    <div class="text-red-400 text-sm bg-red-900/20 border border-red-800 rounded-lg p-3">{error}</div>
  {:else if filteredPrefixes.length === 0}
    <p class="text-gray-500 text-sm">No path status data available. Deploy a BMP speaker that sends Path Status TLVs.</p>
  {:else}
    <!-- Redundancy matrix table -->
    <div class="overflow-x-auto rounded-xl border border-gray-700">
      <table data-testid="path-status-table" class="w-full text-xs">
        <thead>
          <tr class="bg-gray-800/80 text-gray-400 text-left">
            <th class="px-3 py-2 font-medium sticky left-0 bg-gray-800 z-10 w-44">Prefix</th>
            <th class="px-2 py-2 font-medium">Active</th>
            {#each allPeers as peer}
              <th class="px-2 py-2 font-medium whitespace-nowrap">{peer}</th>
            {/each}
          </tr>
        </thead>
        <tbody>
          {#each filteredPrefixes as prefix}
            {@const peerMap = byPrefix.get(prefix)!}
            {@const ap = activePaths(peerMap)}
            <tr class="border-t border-gray-800 hover:bg-gray-800/40 {healthClass(ap)}">
              <!-- Prefix -->
              <td class="px-3 py-1.5 font-mono sticky left-0 bg-gray-900 z-10 border-r border-gray-800">
                <a href="/prefixes?q={prefix}" class="text-blue-400 hover:underline">{prefix}</a>
              </td>
              <!-- Active count badge -->
              <td class="px-2 py-1.5 text-center">
                <span class="px-1.5 py-0.5 rounded text-xs font-mono
                  {ap === 0 ? 'bg-red-700 text-white' : ap < 2 ? 'bg-amber-700 text-black' : 'bg-gray-700 text-gray-200'}">
                  {ap}
                </span>
              </td>
              <!-- Per-peer cells -->
              {#each allPeers as peer}
                {@const row = peerMap.get(peer)}
                <td class="px-2 py-1.5 text-center">
                  {#if row}
                    {@const info = statusInfo(row.path_status)}
                    <span
                      class="inline-flex items-center justify-center w-6 h-6 rounded text-xs {info.cls}"
                      title="{info.label}{row.reason_label ? ' — ' + row.reason_label : ''}">
                      {info.icon}
                    </span>
                  {:else}
                    <span class="text-gray-700">—</span>
                  {/if}
                </td>
              {/each}
            </tr>
          {/each}
        </tbody>
      </table>
    </div>

    <p class="text-xs text-gray-600">
      {filteredPrefixes.length} prefixes × {allPeers.length} peers
      {#if minPaths > 0}· warning rows have &lt; {minPaths} active paths{/if}
    </p>
  {/if}
</div>
