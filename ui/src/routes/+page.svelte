<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { api, openEventStream } from '$lib/api';
  import { Activity, Wifi, Shield, TrendingUp, Router, Copy, Check } from 'lucide-svelte';

  type SpeakerRow = {
    addr: string; hostname: string; vendor: string; bmp_state: string;
    peers_up: number; peers_down: number; route_count: number; connected_at: string;
  };
  type SummaryData = {
    speakers: SpeakerRow[]; count: number; total_peers_up: number; total_routes: number;
    has_speakers: boolean; has_active_sessions: boolean;
  };

  let summary      = $state<SummaryData | null>(null);
  let rpki         = $state<Record<string, number>>({});
  let liveEvents   = $state<string[]>([]);
  let health       = $state<'ok' | 'error' | 'loading'>('loading');
  let copied       = $state(false);
  let activeVendor = $state<'IOS-XR' | 'FRRouting' | 'Arista EOS' | 'JunOS'>('IOS-XR');
  let es: EventSource | null = null;

  const bmpHost = 'your-server';
  const bmpPort = 11019;

  const configSnippets: Record<string, string> = {
    'IOS-XR': `bmp server 1
 host ${bmpHost} port ${bmpPort}
!
router bgp 65001
 bmp servers 1
  initial-delay 5
 !
 neighbor <PEER_IP>
  remote-as <PEER_AS>
  bmp-activate server 1
 !`,
    'FRRouting': `router bgp 65001
 bmp targets rustybmp
  bmp connect ${bmpHost} port ${bmpPort} min-retry 1000 max-retry 5000
  bmp monitor ipv4 unicast pre-policy
  bmp monitor ipv4 unicast post-policy
  bmp monitor ipv4 unicast loc-rib
 exit
!`,
    'Arista EOS': `router bgp 65001
 bmp
  server ${bmpHost} port ${bmpPort}
  !
 !
!`,
    'JunOS': `routing-options {
  bmp {
    station rustybmp {
      initiation-message rustybmp;
      connection {
        address ${bmpHost};
        port ${bmpPort};
      }
      monitor enable;
    }
  }
}`,
  };

  async function load() {
    try {
      await api.health();
      health = 'ok';
    } catch { health = 'error'; }
    const [summaryRes, rpkiRes] = await Promise.all([
      api.speakersSummary().catch(() => null),
      api.rpkiStats().catch(() => ({})),
    ]);
    summary = summaryRes;
    rpki    = rpkiRes as Record<string, number>;
  }

  async function copyConfig() {
    await navigator.clipboard.writeText(configSnippets[activeVendor]);
    copied = true;
    setTimeout(() => { copied = false; }, 2000);
  }

  onMount(() => {
    load();
    es = openEventStream((type, data) => {
      const entry = `${new Date().toLocaleTimeString()} [${type}] ${JSON.stringify(data).slice(0, 80)}`;
      liveEvents = [entry, ...liveEvents.slice(0, 49)];
    });
  });

  onDestroy(() => es?.close());

  const validPct = $derived(rpki.valid && rpki.total
    ? Math.round((rpki.valid / rpki.total) * 100) : 0);

  // Homepage state machine
  const isEmpty    = $derived(!summary || (!summary.has_speakers && summary.total_routes === 0));
  const isWaiting  = $derived(!!summary && summary.has_speakers && !summary.has_active_sessions);
  const isActive   = $derived(!!summary && summary.has_active_sessions);
</script>

<div class="p-6 space-y-6">
  <!-- Header -->
  <div class="flex items-center justify-between">
    <h1 class="text-2xl font-bold text-gray-100">Dashboard</h1>
    <span class="flex items-center gap-2 text-sm {health === 'ok' ? 'text-emerald-400' : health === 'error' ? 'text-red-400' : 'text-gray-400'}">
      <span class="inline-block w-2 h-2 rounded-full {health === 'ok' ? 'bg-emerald-400' : health === 'error' ? 'bg-red-400' : 'bg-gray-500'}"></span>
      {health === 'ok' ? 'Connected' : health === 'loading' ? 'Connecting…' : 'Unreachable'}
    </span>
  </div>

  <!-- ── State A: Empty / Onboarding ──────────────────────────────────────── -->
  {#if isEmpty}
  <div data-testid="onboarding-empty-state"
       class="flex flex-col items-center justify-center min-h-[70vh] gap-8">
    <div class="text-center max-w-xl">
      <div class="flex items-center justify-center mb-4">
        <div class="p-4 rounded-2xl bg-sky-500/10 text-sky-400">
          <Router size={40} />
        </div>
      </div>
      <h2 class="text-2xl font-medium text-gray-100 mb-2">Welcome to RustyBMP</h2>
      <p class="text-gray-400">No BMP sessions yet. Configure your router to send BMP data here.</p>
    </div>

    <div data-testid="quick-config-panel" class="w-full max-w-2xl">
      <div class="flex gap-2 mb-3 flex-wrap">
        {#each ['IOS-XR', 'FRRouting', 'Arista EOS', 'JunOS'] as vendor}
          <button data-testid="vendor-tab-{vendor}"
                  onclick={() => activeVendor = vendor as typeof activeVendor}
                  class="px-4 py-1.5 rounded-lg text-sm font-medium transition-colors
                    {activeVendor === vendor
                      ? 'bg-sky-600 text-white'
                      : 'bg-gray-800 text-gray-400 hover:bg-gray-700'}">
            {vendor}
          </button>
        {/each}
      </div>

      <div class="relative">
        <pre data-testid="router-config-snippet"
             class="font-mono text-sm bg-gray-900 border border-gray-700 rounded-xl p-5 overflow-x-auto text-gray-300 leading-relaxed">{configSnippets[activeVendor]}</pre>
        <button onclick={copyConfig}
                class="absolute top-3 right-3 p-2 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-gray-200 transition-colors">
          {#if copied}<Check size={15} class="text-emerald-400" />{:else}<Copy size={15} />{/if}
        </button>
      </div>
      <p class="text-gray-500 text-sm mt-3 text-center">
        BMP collector listening on <code class="text-sky-400">{bmpHost}:{bmpPort}</code>
      </p>
    </div>
  </div>

  <!-- ── State B: Waiting for BMP sessions ──────────────────────────────────── -->
  {:else if isWaiting}
  <div data-testid="waiting-state" class="space-y-4">
    <p class="text-gray-400">Speakers are configured — waiting for BMP sessions to connect.</p>
    {#each (summary?.speakers ?? []) as speaker}
    <div data-testid="speaker-waiting-{speaker.addr}"
         class="bg-gray-900 border border-gray-800 rounded-xl p-5 flex items-center justify-between">
      <div>
        <p class="font-medium text-gray-200">{speaker.hostname || speaker.addr}</p>
        <p class="text-sm text-gray-500">{speaker.addr}</p>
      </div>
      <div class="flex items-center gap-2 text-amber-400 text-sm">
        <span class="inline-block w-2 h-2 rounded-full bg-amber-400 animate-pulse"></span>
        No BMP session
      </div>
    </div>
    {/each}
  </div>

  <!-- ── State C: Full dashboard ─────────────────────────────────────────────── -->
  {:else if isActive}

  <!-- Speaker cards -->
  <section data-testid="speaker-section">
    <h2 class="text-sm font-semibold text-gray-400 mb-3 uppercase tracking-wider">BGP Speakers</h2>
    <div class="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-4">
      {#each (summary?.speakers ?? []) as speaker}
      <div data-testid="speaker-card-{speaker.addr}"
           class="bg-gray-900 rounded-xl border border-gray-800 p-5 space-y-4">
        <div class="flex items-start justify-between">
          <div>
            <p data-testid="speaker-hostname" class="font-medium text-gray-100">
              {speaker.hostname || speaker.addr}
            </p>
            <p data-testid="speaker-addr" class="text-xs text-gray-500 font-mono">{speaker.addr}</p>
            {#if speaker.vendor}
              <span data-testid="speaker-vendor"
                    class="mt-1 inline-block text-xs bg-gray-800 text-gray-400 px-2 py-0.5 rounded">
                {speaker.vendor}
              </span>
            {/if}
          </div>
          <span data-testid="speaker-status"
                class="inline-block w-2.5 h-2.5 rounded-full mt-1
                  {speaker.bmp_state === 'active' ? 'bg-emerald-400' : 'bg-gray-600'}">
          </span>
        </div>
        <div class="grid grid-cols-3 gap-3 text-center">
          <div class="bg-gray-800 rounded-lg p-2">
            <p class="text-lg font-bold text-emerald-400">{speaker.peers_up}</p>
            <p class="text-xs text-gray-500">Peers Up</p>
          </div>
          <div class="bg-gray-800 rounded-lg p-2">
            <p class="text-lg font-bold text-gray-200">{speaker.route_count.toLocaleString()}</p>
            <p class="text-xs text-gray-500">Routes</p>
          </div>
          <div class="bg-gray-800 rounded-lg p-2">
            <p class="text-lg font-bold {speaker.peers_down > 0 ? 'text-red-400' : 'text-gray-400'}">{speaker.peers_down}</p>
            <p class="text-xs text-gray-500">Peers Down</p>
          </div>
        </div>
        <div class="flex gap-3 text-xs">
          <a href="/peers?speaker={speaker.addr}" class="text-sky-400 hover:text-sky-300">View peers →</a>
          <a href="/prefixes?speaker={speaker.addr}" class="text-sky-400 hover:text-sky-300">View routes →</a>
        </div>
      </div>
      {/each}
    </div>
  </section>

  <!-- Summary metric cards -->
  <section data-testid="summary-metrics" class="grid grid-cols-2 xl:grid-cols-4 gap-4">
    <div class="bg-gray-900 rounded-xl border border-gray-800 p-5 flex items-center gap-4">
      <div class="p-2 rounded-lg bg-emerald-500/10 text-emerald-400"><Wifi size={20} /></div>
      <div>
        <p class="text-xs text-gray-500 uppercase tracking-wide">Peers Up</p>
        <p data-testid="dashboard-peers-up-count" class="text-2xl font-bold text-gray-100">{summary?.total_peers_up ?? 0}</p>
      </div>
    </div>
    <div class="bg-gray-900 rounded-xl border border-gray-800 p-5 flex items-center gap-4">
      <div class="p-2 rounded-lg bg-sky-500/10 text-sky-400"><TrendingUp size={20} /></div>
      <div>
        <p class="text-xs text-gray-500 uppercase tracking-wide">Total Routes</p>
        <p data-testid="dashboard-total-routes" class="text-2xl font-bold text-gray-100">{(summary?.total_routes ?? 0).toLocaleString()}</p>
      </div>
    </div>
    <div class="bg-gray-900 rounded-xl border border-gray-800 p-5 flex items-center gap-4">
      <div class="p-2 rounded-lg bg-violet-500/10 text-violet-400"><Activity size={20} /></div>
      <div>
        <p class="text-xs text-gray-500 uppercase tracking-wide">Speakers</p>
        <p class="text-2xl font-bold text-gray-100">{summary?.count ?? 0}</p>
      </div>
    </div>
    <div class="bg-gray-900 rounded-xl border border-gray-800 p-5 flex items-center gap-4">
      <div class="p-2 rounded-lg bg-sky-500/10 text-sky-400"><Shield size={20} /></div>
      <div>
        <p class="text-xs text-gray-500 uppercase tracking-wide">RPKI Valid %</p>
        <p data-testid="dashboard-rpki-valid" class="text-2xl font-bold text-gray-100">{validPct}%</p>
      </div>
    </div>
  </section>

  <!-- Live event feed -->
  <section data-testid="live-events">
    <h2 class="text-sm font-semibold text-gray-400 mb-2 uppercase tracking-wider">Live Events</h2>
    <div class="bg-gray-900 rounded-xl border border-gray-800 h-64 overflow-y-auto font-mono text-xs p-4 space-y-1">
      {#if liveEvents.length === 0}
        <p class="text-gray-600 italic">Waiting for events…</p>
      {/if}
      {#each liveEvents as ev}
        <div class="text-gray-300">{ev}</div>
      {/each}
    </div>
  </section>

  {/if}
</div>

