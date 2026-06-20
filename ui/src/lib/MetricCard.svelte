<script lang="ts">
  /** RV6-3: MetricCard — single KPI tile with optional trend arrow. */
  export let label:  string;
  export let value:  string | number;
  export let unit:   string  = '';
  export let trend:  number  = 0;   // positive = up, negative = down, 0 = neutral
  export let color:  'blue' | 'green' | 'red' | 'yellow' | 'purple' = 'blue';
  export let loading: boolean = false;

  const colorMap: Record<string, string> = {
    blue:   'bg-blue-900/40 border-blue-700/50 text-blue-300',
    green:  'bg-green-900/40 border-green-700/50 text-green-300',
    red:    'bg-red-900/40 border-red-700/50 text-red-300',
    yellow: 'bg-yellow-900/40 border-yellow-700/50 text-yellow-300',
    purple: 'bg-purple-900/40 border-purple-700/50 text-purple-300',
  };

  $: trendIcon  = trend > 0 ? '▲' : trend < 0 ? '▼' : '—';
  $: trendColor = trend > 0 ? 'text-green-400' : trend < 0 ? 'text-red-400' : 'text-gray-400';
  $: cardClass  = colorMap[color] ?? colorMap.blue;
</script>

<div class="rounded-xl border p-4 flex flex-col gap-1 {cardClass}">
  <span class="text-xs font-medium uppercase tracking-wider opacity-70">{label}</span>
  {#if loading}
    <div class="h-8 w-24 bg-white/10 rounded animate-pulse mt-1" />
  {:else}
    <div class="flex items-baseline gap-1 mt-1">
      <span class="text-2xl font-bold text-white">{value}</span>
      {#if unit}
        <span class="text-sm opacity-60">{unit}</span>
      {/if}
    </div>
  {/if}
  {#if trend !== 0}
    <span class="text-xs {trendColor} mt-0.5">{trendIcon} {Math.abs(trend)}%</span>
  {/if}
</div>
