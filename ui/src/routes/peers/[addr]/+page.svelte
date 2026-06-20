<script lang="ts">
  import { page } from '$app/stores';
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import { Network, Clock, TrendingUp, TrendingDown, ChevronRight, RefreshCw } from 'lucide-svelte';

  const addr = $page.params.addr!;

  type SessionEvent = { occurred_at: string; event_type: string; reason: string | null };
  type TimelineResp = { peer_addr: string; timeline: SessionEvent[] };

  let timeline: SessionEvent[] = $state([]);
  let peerInfo: Record<string, unknown> | null = $state(null);
  let loading = $state(true);
  let error   = $state('');
  let days    = $state(7);

  async function load() {
    loading = true; error = '';
    try {
      const [tl, info] = await Promise.all([
        api.peerTimeline(addr, days) as Promise<TimelineResp>,
        fetch(`/api/peers/${encodeURIComponent(addr)}`).then(r => r.json()),
      ]);
      timeline = tl.timeline ?? [];
      peerInfo = info;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  onMount(load);

  // Derived stats
  type Session = { up: string; down: string | null; dur_secs: number | null };
  const upCount   = $derived(timeline.filter(e => e.event_type === 'peer_up').length);
  const downCount = $derived(timeline.filter(e => e.event_type === 'peer_down').length);

  const sessions = $derived((() => {
    const result: Session[] = [];
    let lastUp: string | null = null;
    for (const e of [...timeline].reverse()) {
      if (e.event_type === 'peer_up')   { lastUp = e.occurred_at; }
      if (e.event_type === 'peer_down' && lastUp) {
        const dur = (new Date(e.occurred_at).getTime() - new Date(lastUp).getTime()) / 1000;
        result.push({ up: lastUp, down: e.occurred_at, dur_secs: dur });
        lastUp = null;
      }
    }
    if (lastUp) result.push({ up: lastUp, down: null, dur_secs: null });
    return result;
  })());

  const longestSession = $derived(sessions.reduce((m, s) => Math.max(m, s.dur_secs ?? 0), 0));

  function fmt(dt: string) { return new Date(dt).toLocaleString(); }
  function fmtDur(s: number | null) {
    if (s == null) return 'ongoing';
    if (s < 60)    return `${Math.round(s)}s`;
    if (s < 3600)  return `${Math.round(s/60)}m`;
    return `${(s/3600).toFixed(1)}h`;
  }
</script>

<svelte:head>
  <title>{addr} — Peer Timeline — RustyBMP</title>
</svelte:head>

<div class="p-6 space-y-6">
  <!-- Header -->
  <div class="flex items-center justify-between">
    <div>
      <div class="flex items-center gap-2 text-sm text-gray-500 mb-1">
        <a href="/peers" class="hover:text-emerald-400">Peers</a>
        <ChevronRight size={12} />
        <span class="font-mono text-gray-300">{addr}</span>
      </div>
      <h1 class="text-2xl font-bold text-gray-100 font-mono">{addr}</h1>
      {#if peerInfo}
        <div class="flex gap-4 text-xs text-gray-500 mt-1">
          <span>AS{(peerInfo as { asn?: number }).asn}</span>
          <span>BGP-ID: {(peerInfo as { bgp_id?: string }).bgp_id}</span>
          <span class="capitalize">{(peerInfo as { state?: string }).state}</span>
        </div>
      {/if}
    </div>
    <div class="flex items-center gap-3">
      <select bind:value={days} on:change={load}
        class="bg-gray-800 border border-gray-700 text-gray-300 text-sm rounded px-3 py-1.5">
        <option value={1}>Last 24h</option>
        <option value={3}>Last 3d</option>
        <option value={7}>Last 7d</option>
        <option value={30}>Last 30d</option>
      </select>
      <button on:click={load}
        class="flex items-center gap-1.5 px-3 py-1.5 bg-gray-800 hover:bg-gray-700 text-gray-300 text-sm rounded border border-gray-700">
        <RefreshCw size={13} /> Refresh
      </button>
    </div>
  </div>

  {#if error}
    <div class="bg-red-900/30 border border-red-700 text-red-300 rounded p-4 text-sm">{error}</div>
  {/if}

  {#if loading}
    <div class="text-gray-500 text-sm animate-pulse">Loading peer timeline…</div>
  {:else}
    <!-- Summary cards -->
    <div class="grid grid-cols-4 gap-4">
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <div class="text-xs text-gray-500 mb-1 uppercase tracking-wide">Sessions Up</div>
        <div class="text-2xl font-bold text-emerald-400 flex items-center gap-1">
          <TrendingUp size={18} />{upCount}
        </div>
        <div class="text-xs text-gray-600 mt-1">last {days}d</div>
      </div>
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <div class="text-xs text-gray-500 mb-1 uppercase tracking-wide">Sessions Down</div>
        <div class="text-2xl font-bold text-red-400 flex items-center gap-1">
          <TrendingDown size={18} />{downCount}
        </div>
        <div class="text-xs text-gray-600 mt-1">flaps</div>
      </div>
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <div class="text-xs text-gray-500 mb-1 uppercase tracking-wide">Total Sessions</div>
        <div class="text-2xl font-bold text-blue-400">{sessions.length}</div>
        <div class="text-xs text-gray-600 mt-1">complete + ongoing</div>
      </div>
      <div class="bg-gray-900 border border-gray-800 rounded-lg p-4">
        <div class="text-xs text-gray-500 mb-1 uppercase tracking-wide">Longest Session</div>
        <div class="text-2xl font-bold text-yellow-400 flex items-center gap-1">
          <Clock size={18} />{fmtDur(longestSession || null)}
        </div>
        <div class="text-xs text-gray-600 mt-1">uptime</div>
      </div>
    </div>

    <!-- Session swim-lane chart -->
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-5">
      <h2 class="text-sm font-semibold text-gray-300 mb-4">Session History</h2>
      {#if sessions.length === 0}
        <div class="text-gray-600 text-sm">No session events in this period.</div>
      {:else}
        <div class="space-y-2">
          {#each sessions as sess, i}
            <div class="flex items-center gap-3">
              <div class="w-5 h-5 rounded-full text-xs flex items-center justify-center font-mono
                          {sess.down ? 'bg-red-900/50 text-red-400' : 'bg-emerald-900/50 text-emerald-400'}">
                {i + 1}
              </div>
              <div class="flex-1 min-w-0">
                <div class="flex items-center justify-between text-xs">
                  <span class="text-gray-400">{fmt(sess.up)}</span>
                  <span class="font-medium {sess.down ? 'text-red-400' : 'text-emerald-400'}">
                    {fmtDur(sess.dur_secs)}
                  </span>
                  {#if sess.down}
                    <span class="text-gray-500">{fmt(sess.down)}</span>
                  {:else}
                    <span class="text-emerald-500">ongoing</span>
                  {/if}
                </div>
                <div class="mt-1 h-2 bg-gray-800 rounded-full overflow-hidden">
                  {#if sess.dur_secs != null && longestSession > 0}
                    <div class="h-2 bg-emerald-600 rounded-full"
                         style="width:{Math.round(sess.dur_secs / longestSession * 100)}%"></div>
                  {:else}
                    <div class="h-2 bg-emerald-500/40 rounded-full w-full animate-pulse"></div>
                  {/if}
                </div>
              </div>
            </div>
          {/each}
        </div>
      {/if}
    </div>

    <!-- Raw event log -->
    <div class="bg-gray-900 border border-gray-800 rounded-lg p-5">
      <h2 class="text-sm font-semibold text-gray-300 mb-4">Event Log</h2>
      {#if timeline.length === 0}
        <div class="text-gray-600 text-sm">No events recorded.</div>
      {:else}
        <div class="overflow-x-auto">
          <table class="w-full text-xs text-left">
            <thead>
              <tr class="text-gray-500 border-b border-gray-800">
                <th class="pb-2 pr-6">Time</th>
                <th class="pb-2 pr-6">Event</th>
                <th class="pb-2">Reason</th>
              </tr>
            </thead>
            <tbody>
              {#each timeline as evt}
                <tr class="border-b border-gray-800/50 hover:bg-gray-800/30">
                  <td class="py-1.5 pr-6 font-mono text-gray-400">{fmt(evt.occurred_at)}</td>
                  <td class="py-1.5 pr-6">
                    <span class="px-1.5 py-0.5 rounded text-xs font-medium
                      {evt.event_type === 'peer_up'
                        ? 'bg-emerald-900/40 text-emerald-400'
                        : 'bg-red-900/40 text-red-400'}">
                      {evt.event_type}
                    </span>
                  </td>
                  <td class="py-1.5 text-gray-500">{evt.reason ?? '—'}</td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      {/if}
    </div>
  {/if}
</div>
