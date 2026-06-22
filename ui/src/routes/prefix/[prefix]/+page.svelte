<script lang="ts">
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { Shield, Clock, Network, ChevronRight, TrendingUp, TrendingDown, RefreshCw } from 'lucide-svelte';

  const prefix = $page.params.prefix!;

  let timeline: { bucket: string; action: string; count: number }[] = [];
  let peers: Record<string, unknown>[] = [];
  let convergenceEvents: Record<string, unknown>[] = [];
  let history: Record<string, unknown>[] = [];
  let loading = true;
  let error = '';
  let days = 7;

  async function load() {
    loading = true;
    error = '';
    try {
      const [tl, pl, cv] = await Promise.all([
        api.prefixTimeline(prefix, days),
        api.prefixPeers(prefix),
        api.prefixConvergence(prefix),
      ]);
      timeline = tl.timeline;
      peers = pl.peers as Record<string, unknown>[];
      convergenceEvents = cv.events as Record<string, unknown>[];
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  // Build timeline chart — bucket announces and withdraws by hour
  $: announceByBucket = Object.fromEntries(
    timeline.filter(t => t.action === 'announce').map(t => [t.bucket, t.count])
  );
  $: withdrawByBucket = Object.fromEntries(
    timeline.filter(t => t.action === 'withdraw').map(t => [t.bucket, t.count])
  );
  $: allBuckets = [...new Set(timeline.map(t => t.bucket))].sort();
  $: maxCount = Math.max(1, ...timeline.map(t => t.count));

  // Summary stats
  $: totalAnnounce = timeline.filter(t => t.action === 'announce').reduce((s, t) => s + t.count, 0);
  $: totalWithdraw = timeline.filter(t => t.action === 'withdraw').reduce((s, t) => s + t.count, 0);
  $: peerCount = peers.length;

  // Most recent convergence
  $: lastGap = (convergenceEvents.find(e => (e as Record<string,unknown>).gap_secs != null) as Record<string,unknown> | undefined)?.gap_secs as number | undefined;

  function fmt(dt: string) {
    return new Date(dt).toLocaleString();
  }

  function fmtSecs(s: number | undefined) {
    if (s == null) return '—';
    if (s < 1) return `${Math.round(s * 1000)}ms`;
    return `${s.toFixed(1)}s`;
  }
</script>

<svelte:head>
  <title>{prefix} — Prefix Explorer — RustyBMP</title>
</svelte:head>

<div data-testid="page-prefix-detail" class="p-6 space-y-6">
  <!-- Header -->
  <div class="flex items-center justify-between">
    <div>
      <div class="flex items-center gap-2 text-sm text-gray-500 mb-1">
        <a href="/prefixes" class="hover:text-emerald-400">Prefixes</a>
        <ChevronRight size={12} />
        <span class="text-gray-300 font-mono">{prefix}</span>
      </div>
      <h1 class="text-2xl font-bold text-gray-100 font-mono">{prefix}</h1>
    </div>
    <div class="flex items-center gap-3">
      <select
        data-testid="prefix-detail-days"
        bind:value={days}
        on:change={load}
        class="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded px-3 py-1.5"
      >
        <option value={1}>Last 24h</option>
        <option value={3}>Last 3d</option>
        <option value={7}>Last 7d</option>
        <option value={30}>Last 30d</option>
      </select>
      <button
        data-testid="prefix-detail-refresh"
        on:click={load}
        class="flex items-center gap-1.5 px-3 py-1.5 bg-gray-800 hover:bg-gray-700 text-gray-300 text-sm rounded border border-gray-700"
      >
        <RefreshCw size={13} />
        Refresh
      </button>
    </div>
  </div>

  {#if error}
    <div class="bg-red-900/30 border border-red-700 text-red-300 rounded p-4 text-sm">{error}</div>
  {/if}

  {#if loading}
    <div class="text-gray-500 text-sm animate-pulse">Loading prefix data…</div>
  {:else}
    <!-- Summary cards -->
    <div class="grid grid-cols-4 gap-4">
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <div class="text-xs text-gray-500 mb-1 uppercase tracking-wide">Announces</div>
        <div class="text-2xl font-bold text-emerald-400 flex items-center gap-1">
          <TrendingUp size={18} />{totalAnnounce}
        </div>
        <div class="text-xs text-gray-600 mt-1">last {days}d</div>
      </div>
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <div class="text-xs text-gray-500 mb-1 uppercase tracking-wide">Withdrawals</div>
        <div class="text-2xl font-bold text-red-400 flex items-center gap-1">
          <TrendingDown size={18} />{totalWithdraw}
        </div>
        <div class="text-xs text-gray-600 mt-1">last {days}d</div>
      </div>
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <div class="text-xs text-gray-500 mb-1 uppercase tracking-wide">Peers Seeing</div>
        <div class="text-2xl font-bold text-blue-400 flex items-center gap-1">
          <Network size={18} />{peerCount}
        </div>
        <div class="text-xs text-gray-600 mt-1">active peers</div>
      </div>
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <div class="text-xs text-gray-500 mb-1 uppercase tracking-wide">Last Convergence</div>
        <div class="text-2xl font-bold text-yellow-400 flex items-center gap-1">
          <Clock size={18} />{fmtSecs(lastGap)}
        </div>
        <div class="text-xs text-gray-600 mt-1">between events</div>
      </div>
    </div>

    <!-- Timeline chart -->
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-5">
      <h2 class="text-sm font-semibold text-gray-300 mb-4">Announcement Timeline (last {days}d)</h2>
      {#if allBuckets.length === 0}
        <div class="text-gray-600 text-sm">No events in this period.</div>
      {:else}
        <div class="overflow-x-auto">
          <div class="flex items-end gap-1 h-28 min-w-0">
            {#each allBuckets as bucket}
              {@const ann = announceByBucket[bucket] ?? 0}
              {@const wd = withdrawByBucket[bucket] ?? 0}
              <div class="flex flex-col items-center gap-0.5 flex-1 min-w-[6px]" title={bucket}>
                {#if ann > 0}
                  <div
                    class="w-full bg-emerald-500 rounded-t"
                    style="height:{Math.round((ann / maxCount) * 80)}px"
                  ></div>
                {/if}
                {#if wd > 0}
                  <div
                    class="w-full bg-red-500"
                    style="height:{Math.round((wd / maxCount) * 80)}px"
                  ></div>
                {/if}
              </div>
            {/each}
          </div>
          <div class="flex justify-between text-xs text-gray-600 mt-1">
            <span>{fmt(allBuckets[0])}</span>
            <span>{fmt(allBuckets[allBuckets.length - 1])}</span>
          </div>
        </div>
        <div class="flex gap-4 mt-3 text-xs text-gray-500">
          <span class="flex items-center gap-1"><span class="w-3 h-3 bg-emerald-500 rounded-sm inline-block"></span>Announce</span>
          <span class="flex items-center gap-1"><span class="w-3 h-3 bg-red-500 rounded-sm inline-block"></span>Withdraw</span>
        </div>
      {/if}
    </div>

    <!-- AS Path per peer -->
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-5">
      <h2 class="text-sm font-semibold text-gray-300 mb-4">AS Path per Peer</h2>
      {#if peers.length === 0}
        <div class="text-gray-600 text-sm">No peers currently announcing this prefix.</div>
      {:else}
        <div class="space-y-3">
          {#each peers as peer}
            {@const p = peer as { peer_addr: string; peer_as: number; as_path: string | null; next_hop: string | null; local_pref: number | null; communities: string | null; last_seen: string }}
            <div class="bg-gray-800/50 rounded-lg p-3">
              <div class="flex items-center justify-between mb-1">
                <span class="text-sm font-mono text-blue-400">{p.peer_addr}</span>
                <span class="text-xs text-gray-500">AS{p.peer_as}</span>
              </div>
              <div class="font-mono text-xs text-gray-300">
                {#if p.as_path}
                  {#each p.as_path.split(' ') as asn, i}
                    <span class="inline-flex items-center">
                      {#if i > 0}<span class="text-gray-600 mx-1">→</span>{/if}
                      <span class="px-1.5 py-0.5 bg-gray-700 rounded text-emerald-300">{asn}</span>
                    </span>
                  {/each}
                {:else}
                  <span class="text-gray-600">—</span>
                {/if}
              </div>
              <div class="flex gap-4 mt-2 text-xs text-gray-500">
                {#if p.next_hop}<span>next-hop: <span class="text-gray-300">{p.next_hop}</span></span>{/if}
                {#if p.local_pref != null}<span>LP: <span class="text-gray-300">{p.local_pref}</span></span>{/if}
                {#if p.communities}<span>communities: <span class="text-gray-300 font-mono">{p.communities}</span></span>{/if}
                <span class="ml-auto">last seen: {fmt(p.last_seen)}</span>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Event history -->
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-5">
      <h2 class="text-sm font-semibold text-gray-300 mb-4">Convergence Events</h2>
      {#if convergenceEvents.length === 0}
        <div class="text-gray-600 text-sm">No events recorded.</div>
      {:else}
        <div class="overflow-x-auto">
          <table class="w-full text-xs text-left">
            <thead>
              <tr class="text-gray-500 border-b border-gray-800">
                <th class="pb-2 pr-4">Time</th>
                <th class="pb-2 pr-4">Peer</th>
                <th class="pb-2 pr-4">Action</th>
                <th class="pb-2">Gap to prev</th>
              </tr>
            </thead>
            <tbody>
              {#each convergenceEvents as evt}
                {@const e = evt as { occurred_at: string; peer_addr: string; action: string; gap_secs: number | null }}
                <tr class="border-b border-gray-800/50 hover:bg-gray-800/30">
                  <td class="py-1.5 pr-4 font-mono text-gray-400">{fmt(e.occurred_at)}</td>
                  <td class="py-1.5 pr-4 font-mono text-blue-400">{e.peer_addr}</td>
                  <td class="py-1.5 pr-4">
                    <span class="px-1.5 py-0.5 rounded text-xs font-medium {e.action === 'announce' ? 'bg-emerald-900/40 text-emerald-400' : 'bg-red-900/40 text-red-400'}">
                      {e.action}
                    </span>
                  </td>
                  <td class="py-1.5 text-yellow-400 font-mono">
                    {fmtSecs(e.gap_secs ?? undefined)}
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  {/if}
</div>
