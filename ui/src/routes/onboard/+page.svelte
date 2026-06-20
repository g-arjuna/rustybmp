<script lang="ts">
  import { CheckCircle, Circle, AlertCircle, Server, Network, Shield, Loader } from 'lucide-svelte';

  const BASE = '';

  type StepStatus = 'idle' | 'loading' | 'ok' | 'error';

  let addr    = $state('');
  let step    = $state(0);  // 0 = not started, 1-4
  let results = $state<Record<number, { status: StepStatus; data: unknown }>>({});

  // Step 2 inputs
  let hostname = $state('');
  let vendor   = $state('');
  let site     = $state('');

  // Step 3 inputs
  let filterYaml = $state('');
  let skipFilter = $state(false);

  function setResult(s: number, status: StepStatus, data: unknown) {
    results = { ...results, [s]: { status, data } };
  }

  async function doStep(n: number) {
    setResult(n, 'loading', null);
    step = n;
    try {
      let res: Response;
      if (n === 1) {
        res = await fetch(`${BASE}/api/onboard/${encodeURIComponent(addr)}/validate`);
      } else if (n === 2) {
        res = await fetch(`${BASE}/api/onboard/${encodeURIComponent(addr)}/register`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ hostname, vendor, site }),
        });
      } else if (n === 3) {
        if (skipFilter) {
          setResult(3, 'ok', { message: 'Filter step skipped.' });
          return;
        }
        res = await fetch(`${BASE}/api/onboard/${encodeURIComponent(addr)}/filter`, {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ filter_yaml: filterYaml }),
        });
      } else {
        res = await fetch(`${BASE}/api/onboard/${encodeURIComponent(addr)}/confirm`);
      }
      const data = await res!.json();
      setResult(n, res!.ok ? 'ok' : 'error', data);
    } catch (e) {
      setResult(n, 'error', { message: String(e) });
    }
  }

  const steps = [
    { n: 1, label: 'Validate Connectivity', icon: Network },
    { n: 2, label: 'Register Metadata',     icon: Server },
    { n: 3, label: 'Apply Filter Rules',    icon: Shield },
    { n: 4, label: 'Confirm Onboarding',    icon: CheckCircle },
  ];

  function stepColor(n: number): string {
    const r = results[n];
    if (!r) return 'text-gray-600';
    if (r.status === 'loading') return 'text-yellow-400';
    if (r.status === 'ok') return 'text-emerald-400';
    return 'text-red-400';
  }

  function stepBg(n: number): string {
    const r = results[n];
    if (!r) return 'bg-gray-800 border-gray-700';
    if (r.status === 'ok') return 'bg-emerald-900/20 border-emerald-700/40';
    if (r.status === 'error') return 'bg-red-900/20 border-red-700/40';
    return 'bg-gray-800 border-gray-700';
  }

  const step1data = $derived(results[1]?.data as { status?: string; message?: string; peers_up?: number } | undefined);
  const step4data = $derived(results[4]?.data as { status?: string; peers?: unknown[]; total_routes?: number; message?: string } | undefined);
</script>

<svelte:head>
  <title>Speaker Onboarding — RustyBMP</title>
</svelte:head>

<div class="p-6 max-w-3xl mx-auto space-y-6">
  <div>
    <h1 class="text-2xl font-bold text-gray-100">Speaker Onboarding</h1>
    <p class="text-gray-500 text-sm mt-1">4-step wizard to connect and configure a new BMP speaker.</p>
  </div>

  <!-- Address input -->
  <div class="bg-gray-900 border border-gray-800 rounded-lg p-5 space-y-3">
    <label class="block text-sm text-gray-400">BMP Speaker IP Address</label>
    <div class="flex gap-2">
      <input
        bind:value={addr}
        placeholder="192.0.2.1"
        class="flex-1 bg-gray-800 border border-gray-700 rounded px-3 py-2 text-sm text-gray-200
               placeholder-gray-600 focus:outline-none focus:border-emerald-500 font-mono"
      />
      <button
        disabled={!addr}
        on:click={() => doStep(1)}
        class="px-4 py-2 bg-emerald-600 hover:bg-emerald-500 disabled:opacity-40 text-white text-sm rounded font-medium"
      >
        Start
      </button>
    </div>
  </div>

  <!-- Progress bar -->
  <div class="flex items-center gap-0">
    {#each steps as s, i}
      {@const done = results[s.n]?.status === 'ok'}
      {@const active = step === s.n}
      <div class="flex-1 flex flex-col items-center">
        <div class="w-8 h-8 rounded-full flex items-center justify-center text-xs font-bold
                    {done ? 'bg-emerald-600 text-white' : active ? 'bg-gray-700 text-emerald-400 ring-2 ring-emerald-500' : 'bg-gray-800 text-gray-500'}">
          {#if done}<CheckCircle size={16} />{:else}{s.n}{/if}
        </div>
        <div class="text-xs mt-1 text-center {done ? 'text-emerald-400' : active ? 'text-gray-300' : 'text-gray-600'}">{s.label}</div>
      </div>
      {#if i < steps.length - 1}
        <div class="flex-1 h-px {results[s.n]?.status === 'ok' ? 'bg-emerald-600' : 'bg-gray-800'} mb-5"></div>
      {/if}
    {/each}
  </div>

  <!-- Step panels -->

  <!-- Step 1: Validate -->
  {#if step >= 1}
    <div class="bg-gray-900 border {stepBg(1)} rounded-lg p-5 space-y-3">
      <div class="flex items-center gap-2 {stepColor(1)} font-semibold text-sm">
        <Network size={16} /> Step 1 — Validate Connectivity
      </div>
      {#if results[1]?.status === 'loading'}
        <div class="text-yellow-400 text-sm animate-pulse flex items-center gap-2"><Loader size={14} />Checking…</div>
      {:else if results[1]?.status === 'ok'}
        <div class="text-sm text-gray-300">{step1data?.message}</div>
        {#if step1data?.status === 'connected'}
          <div class="text-xs text-gray-500">Peers up: {step1data.peers_up ?? 0}</div>
        {/if}
        {#if step === 1}
          <button on:click={() => doStep(2)}
            class="mt-2 px-4 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded">
            Next: Register →
          </button>
        {/if}
      {:else if results[1]?.status === 'error'}
        <div class="text-red-400 text-sm">{(results[1].data as { message?: string })?.message ?? 'Error'}</div>
      {/if}
    </div>
  {/if}

  <!-- Step 2: Register -->
  {#if step >= 2}
    <div class="bg-gray-900 border {stepBg(2)} rounded-lg p-5 space-y-3">
      <div class="flex items-center gap-2 {stepColor(2)} font-semibold text-sm">
        <Server size={16} /> Step 2 — Register Metadata
      </div>
      {#if results[2]?.status !== 'ok'}
        <div class="grid grid-cols-3 gap-3">
          <div>
            <label class="block text-xs text-gray-500 mb-1">Hostname</label>
            <input bind:value={hostname} placeholder="router-1.example.com"
              class="w-full bg-gray-800 border border-gray-700 rounded px-2 py-1.5 text-sm text-gray-200 focus:outline-none focus:border-emerald-500" />
          </div>
          <div>
            <label class="block text-xs text-gray-500 mb-1">Vendor</label>
            <input bind:value={vendor} placeholder="Cisco, Juniper…"
              class="w-full bg-gray-800 border border-gray-700 rounded px-2 py-1.5 text-sm text-gray-200 focus:outline-none focus:border-emerald-500" />
          </div>
          <div>
            <label class="block text-xs text-gray-500 mb-1">Site</label>
            <input bind:value={site} placeholder="NYC-DC1"
              class="w-full bg-gray-800 border border-gray-700 rounded px-2 py-1.5 text-sm text-gray-200 focus:outline-none focus:border-emerald-500" />
          </div>
        </div>
        <button on:click={() => doStep(2)}
          class="px-4 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded">
          Register
        </button>
      {:else}
        <div class="text-sm text-emerald-400">✓ Metadata registered</div>
        {#if step === 2}
          <button on:click={() => doStep(3)}
            class="mt-1 px-4 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded">
            Next: Filter →
          </button>
        {/if}
      {/if}
    </div>
  {/if}

  <!-- Step 3: Filter -->
  {#if step >= 3}
    <div class="bg-gray-900 border {stepBg(3)} rounded-lg p-5 space-y-3">
      <div class="flex items-center gap-2 {stepColor(3)} font-semibold text-sm">
        <Shield size={16} /> Step 3 — Apply Filter Rules
      </div>
      {#if results[3]?.status !== 'ok'}
        <label class="flex items-center gap-2 text-sm text-gray-400 cursor-pointer">
          <input type="checkbox" bind:checked={skipFilter} class="accent-emerald-500" />
          Skip — no filter for this speaker
        </label>
        {#if !skipFilter}
          <textarea
            bind:value={filterYaml}
            placeholder="- name: my-filter&#10;  action: deny&#10;  prefixes: [10.0.0.0/8]"
            rows={6}
            class="w-full bg-gray-800 border border-gray-700 rounded px-3 py-2 text-xs text-gray-200
                   font-mono focus:outline-none focus:border-emerald-500 resize-y"
          ></textarea>
        {/if}
        <button on:click={() => doStep(3)}
          class="px-4 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded">
          {skipFilter ? 'Skip' : 'Apply Filter'}
        </button>
      {:else}
        <div class="text-sm text-emerald-400">✓ {(results[3].data as { message?: string })?.message ?? 'Filter applied'}</div>
        {#if step === 3}
          <button on:click={() => doStep(4)}
            class="mt-1 px-4 py-1.5 bg-emerald-600 hover:bg-emerald-500 text-white text-sm rounded">
            Next: Confirm →
          </button>
        {/if}
      {/if}
    </div>
  {/if}

  <!-- Step 4: Confirm -->
  {#if step >= 4}
    <div class="bg-gray-900 border {stepBg(4)} rounded-lg p-5 space-y-3">
      <div class="flex items-center gap-2 {stepColor(4)} font-semibold text-sm">
        <CheckCircle size={16} /> Step 4 — Confirm Onboarding
      </div>
      {#if results[4]?.status === 'loading'}
        <div class="text-yellow-400 text-sm animate-pulse flex items-center gap-2"><Loader size={14} />Verifying…</div>
      {:else if results[4]?.status === 'ok'}
        {#if step4data?.status === 'onboarded'}
          <div class="text-emerald-400 font-semibold">🎉 {step4data.message}</div>
          <div class="grid grid-cols-2 gap-2 text-xs text-gray-400">
            <span>Peers up: <span class="text-gray-200">{step4data.peers?.length ?? 0}</span></span>
            <span>Total routes: <span class="text-gray-200">{step4data.total_routes ?? 0}</span></span>
          </div>
          <a href="/peers"
            class="inline-block mt-1 px-4 py-1.5 bg-gray-700 hover:bg-gray-600 text-gray-200 text-sm rounded">
            View Peers →
          </a>
        {:else}
          <div class="text-yellow-400 text-sm">{step4data?.message}</div>
          <button on:click={() => doStep(4)} class="px-3 py-1 text-xs bg-gray-700 hover:bg-gray-600 text-gray-300 rounded">
            Re-check
          </button>
        {/if}
      {:else if results[4]?.status === 'error'}
        <div class="text-red-400 text-sm">{(results[4].data as { message?: string })?.message ?? 'Error'}</div>
      {/if}
    </div>
  {/if}
</div>
