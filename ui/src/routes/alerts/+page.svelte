<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { openEventStream } from '$lib/api';
  import { Bell, Trash2 } from 'lucide-svelte';

  interface AlertEntry {
    id:          string;
    kind:        string;
    prefix:      string;
    severity:    'warn' | 'critical' | 'info';
    description: string;
    ts:          string;
  }

  let alerts = $state<AlertEntry[]>([]);
  let es: EventSource | null = null;

  function clear() { alerts = []; }

  onMount(() => {
    es = openEventStream((type, data) => {
      if (type === 'route_change' || type === 'alert') {
        const d = data as Record<string, unknown>;
        const entry: AlertEntry = {
          id:          crypto.randomUUID(),
          kind:        (d.kind as string)     ?? type,
          prefix:      (d.prefix as string)   ?? '—',
          severity:    (d.severity as 'warn' | 'critical' | 'info') ?? 'info',
          description: (d.description as string) ?? JSON.stringify(d).slice(0, 120),
          ts:          new Date().toISOString(),
        };
        alerts = [entry, ...alerts.slice(0, 199)];
      }
    });
  });

  onDestroy(() => es?.close());

  const sevColor = (s: string) => ({
    critical: 'bg-red-500/15 text-red-400 border-red-500/30',
    warn:     'bg-amber-500/15 text-amber-400 border-amber-500/30',
    info:     'bg-sky-500/15 text-sky-400 border-sky-500/30',
  }[s] ?? 'bg-gray-800 text-gray-400');
</script>

<div class="p-6 space-y-5">
  <div class="flex items-center justify-between">
    <div class="flex items-center gap-3">
      <h1 class="text-2xl font-bold text-gray-100">Alerts</h1>
      {#if alerts.length > 0}
        <span class="px-2 py-0.5 rounded-full bg-red-500/20 text-red-400 text-xs font-medium">
          {alerts.length}
        </span>
      {/if}
    </div>
    <button
      onclick={clear}
      disabled={alerts.length === 0}
      class="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm
             bg-gray-800 text-gray-400 hover:text-gray-100 hover:bg-gray-700
             disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
    >
      <Trash2 size={13} /> Clear
    </button>
  </div>

  {#if alerts.length === 0}
    <div class="flex flex-col items-center justify-center py-24 text-gray-600">
      <Bell size={36} class="mb-3 opacity-30" />
      <p class="text-sm">No alerts yet — live SSE stream is active.</p>
    </div>
  {:else}
    <div class="space-y-2">
      {#each alerts as alert (alert.id)}
        <div class="bg-gray-900 rounded-xl border border-gray-800 p-4 flex items-start gap-4">
          <span class="mt-0.5 px-2 py-0.5 rounded border text-xs font-medium flex-shrink-0 {sevColor(alert.severity)}">
            {alert.severity}
          </span>
          <div class="flex-1 min-w-0">
            <div class="flex items-center gap-2 mb-0.5">
              <span class="text-xs font-semibold text-gray-300">{alert.kind}</span>
              <span class="font-mono text-xs text-emerald-400">{alert.prefix}</span>
            </div>
            <p class="text-sm text-gray-400 break-words">{alert.description}</p>
          </div>
          <span class="text-xs text-gray-600 flex-shrink-0">
            {new Date(alert.ts).toLocaleTimeString()}
          </span>
        </div>
      {/each}
    </div>
  {/if}
</div>
