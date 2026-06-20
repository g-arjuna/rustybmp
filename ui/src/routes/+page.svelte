<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { api, openEventStream, type RouteRow } from '$lib/api';
  import { Activity, Wifi, Shield, TrendingUp } from 'lucide-svelte';

  let peers        = $state<{ peer_addr: string; state: string }[]>([]);
  let speakers     = $state<{ addr: string }[]>([]);
  let rpki         = $state<Record<string, number>>({});
  let liveEvents   = $state<string[]>([]);
  let health       = $state<'ok' | 'error' | 'loading'>('loading');
  let es: EventSource | null = null;

  async function load() {
    try {
      await api.health();
      health = 'ok';
    } catch { health = 'error'; }
    [peers, speakers, rpki] = await Promise.all([
      api.peers().catch(() => []),
      api.speakers().catch(() => []),
      api.rpkiStats().catch(() => ({})),
    ]);
  }

  onMount(() => {
    load();
    es = openEventStream((type, data) => {
      const entry = `${new Date().toLocaleTimeString()} [${type}] ${JSON.stringify(data).slice(0, 80)}`;
      liveEvents = [entry, ...liveEvents.slice(0, 49)];
    });
  });

  onDestroy(() => es?.close());

  const upPeers   = $derived(peers.filter(p => p.state === 'up').length);
  const downPeers = $derived(peers.filter(p => p.state !== 'up').length);
  const validPct  = $derived(rpki.valid && rpki.total ? Math.round((rpki.valid / rpki.total) * 100) : 0);
</script>

<div class="p-6 space-y-6">
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100">Dashboard</h1>
    <span class="flex items-center gap-2 text-sm {health === 'ok' ? 'text-emerald-400' : 'text-red-400'}">
      <span class="inline-block w-2 h-2 rounded-full {health === 'ok' ? 'bg-emerald-400' : 'bg-red-400'}"></span>
      {health === 'ok' ? 'Connected' : health === 'loading' ? 'Connecting…' : 'Unreachable'}
    </span>
  </div>

  <!-- Stat cards -->
  <div class="grid grid-cols-2 xl:grid-cols-4 gap-4">
    <div class="bg-gray-900 rounded-xl border border-gray-800 p-5 flex items-center gap-4">
      <div class="p-2 rounded-lg bg-emerald-500/10 text-emerald-400"><Wifi size={20} /></div>
      <div><p class="text-xs text-gray-500 uppercase tracking-wide">Peers Up</p>
        <p class="text-2xl font-bold text-gray-100">{upPeers}</p></div>
    </div>
    <div class="bg-gray-900 rounded-xl border border-gray-800 p-5 flex items-center gap-4">
      <div class="p-2 rounded-lg bg-red-500/10 text-red-400"><Activity size={20} /></div>
      <div><p class="text-xs text-gray-500 uppercase tracking-wide">Peers Down</p>
        <p class="text-2xl font-bold text-gray-100">{downPeers}</p></div>
    </div>
    <div class="bg-gray-900 rounded-xl border border-gray-800 p-5 flex items-center gap-4">
      <div class="p-2 rounded-lg bg-sky-500/10 text-sky-400"><Shield size={20} /></div>
      <div><p class="text-xs text-gray-500 uppercase tracking-wide">RPKI Valid %</p>
        <p class="text-2xl font-bold text-gray-100">{validPct}%</p></div>
    </div>
    <div class="bg-gray-900 rounded-xl border border-gray-800 p-5 flex items-center gap-4">
      <div class="p-2 rounded-lg bg-violet-500/10 text-violet-400"><TrendingUp size={20} /></div>
      <div><p class="text-xs text-gray-500 uppercase tracking-wide">Speakers</p>
        <p class="text-2xl font-bold text-gray-100">{speakers.length}</p></div>
    </div>
  </div>

  <!-- Live event feed -->
  <div>
    <h2 class="text-sm font-semibold text-gray-400 mb-2 uppercase tracking-wider">Live Events</h2>
    <div class="bg-gray-900 rounded-xl border border-gray-800 h-72 overflow-y-auto font-mono text-xs p-4 space-y-1">
      {#if liveEvents.length === 0}
        <p class="text-gray-600 italic">Waiting for events…</p>
      {/if}
      {#each liveEvents as ev}
        <div class="text-gray-300">{ev}</div>
      {/each}
    </div>
  </div>
</div>

