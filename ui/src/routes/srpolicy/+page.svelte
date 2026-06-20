<script lang="ts">
  import { onMount } from 'svelte';
  import { api } from '$lib/api';
  import VirtualTable from '$lib/VirtualTable.svelte';
  import MetricCard from '$lib/MetricCard.svelte';

  let policies: any[] = [];
  let loading = true;
  let error   = '';
  let filter  = '';

  onMount(async () => {
    try {
      const r = await api.srpolicyList(500);
      policies = r.policies as any[];
    } catch (e: any) {
      error = e.message;
    } finally {
      loading = false;
    }
  });

  $: filtered = policies.filter((p: any) =>
    !filter || JSON.stringify(p).toLowerCase().includes(filter.toLowerCase())
  );

  const columns = [
    { key: 'occurred_at',  label: 'Time',       width: '160px' },
    { key: 'peer_addr',    label: 'Peer',        width: '140px' },
    { key: 'peer_as',      label: 'Peer AS',     width: '80px',  right: true },
    { key: 'action',       label: 'Action',      width: '80px' },
    { key: 'endpoint',     label: 'Endpoint',    width: '130px' },
    { key: 'color',        label: 'Color',       width: '70px',  right: true },
    { key: 'preference',   label: 'Pref',        width: '70px',  right: true },
    { key: 'bsid',         label: 'BSID',        width: '140px' },
    { key: 'segment_list', label: 'Segments' },
  ];

  function fmtTime(ts: string): string {
    try { return new Date(ts).toLocaleString(); } catch { return ts; }
  }
</script>

<div class="p-6 max-w-7xl mx-auto space-y-6">
  <div>
    <h1 class="text-2xl font-bold text-white">SR Policy</h1>
    <p class="text-gray-400 text-sm mt-1">BGP-SR Policy NLRIs (RFC 9252 / AFI 1/2 SAFI 73)</p>
  </div>

  {#if !loading}
    <div class="grid grid-cols-3 gap-4">
      <MetricCard label="Total Events"  value={policies.length}         color="blue" />
      <MetricCard label="Announcements" value={policies.filter((p: any) => p.action === 'announce').length} color="green" />
      <MetricCard label="Withdrawals"   value={policies.filter((p: any) => p.action === 'withdraw').length} color="red" />
    </div>
  {/if}

  <div class="flex gap-3">
    <input
      bind:value={filter}
      placeholder="Filter by peer, endpoint, color…"
      class="flex-1 bg-gray-800 border border-gray-700 rounded-lg px-3 py-2 text-sm text-white"
    />
  </div>

  {#if loading}
    <div class="h-64 bg-gray-800/50 rounded-xl animate-pulse" />
  {:else if error}
    <div class="text-red-400 text-sm">{error}</div>
  {:else}
    <VirtualTable rows={filtered} {columns} maxHeight={520} getKey={(r: any) => r.occurred_at + r.peer_addr + r.color}>
      <svelte:fragment slot="cell" let:row let:col>
        {#if col.key === 'occurred_at'}
          <span class="text-gray-500">{fmtTime((row as any).occurred_at)}</span>
        {:else if col.key === 'action'}
          <span class="{(row as any).action === 'announce' ? 'text-green-400' : 'text-red-400'} font-semibold">
            {(row as any).action}
          </span>
        {:else if col.key === 'segment_list'}
          <span class="text-xs">{(row as any).segment_list ?? '—'}</span>
        {:else}
          {(row as any)[col.key] ?? '—'}
        {/if}
      </svelte:fragment>
    </VirtualTable>
  {/if}
</div>
