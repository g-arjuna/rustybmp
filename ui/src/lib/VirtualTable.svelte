<script lang="ts">
  /**
   * RV6-3: VirtualTable — windowed table for large datasets.
   * Renders only visible rows + overscan to keep DOM small.
   */
  import { onMount, tick } from 'svelte';

  export let rows:       unknown[]  = [];
  export let columns:    { key: string; label: string; width?: string; right?: boolean }[] = [];
  export let rowHeight:  number     = 36;
  export let maxHeight:  number     = 480;
  export let getKey:     (row: unknown, i: number) => string | number = (_, i) => i;

  const OVERSCAN = 4;

  let viewport:    HTMLDivElement;
  let scrollTop:   number = 0;
  let viewportH:   number = maxHeight;

  $: totalH      = rows.length * rowHeight;
  $: startIdx    = Math.max(0, Math.floor(scrollTop / rowHeight) - OVERSCAN);
  $: endIdx      = Math.min(rows.length, Math.ceil((scrollTop + viewportH) / rowHeight) + OVERSCAN);
  $: visibleRows = rows.slice(startIdx, endIdx);
  $: offsetTop   = startIdx * rowHeight;
  $: offsetBot   = (rows.length - endIdx) * rowHeight;

  function onScroll(e: Event) {
    scrollTop = (e.target as HTMLDivElement).scrollTop;
  }
</script>

<div
  bind:this={viewport}
  on:scroll={onScroll}
  class="overflow-auto rounded-lg border border-gray-700"
  style="max-height:{maxHeight}px"
>
  <table class="w-full text-sm border-collapse">
    <thead class="sticky top-0 z-10 bg-gray-900">
      <tr>
        {#each columns as col}
          <th
            class="px-3 py-2 text-left text-xs font-semibold uppercase tracking-wider text-gray-400 border-b border-gray-700"
            class:text-right={col.right}
            style={col.width ? `width:${col.width}` : ''}
          >{col.label}</th>
        {/each}
      </tr>
    </thead>
    <tbody>
      {#if offsetTop > 0}
        <tr style="height:{offsetTop}px"><td colspan={columns.length} /></tr>
      {/if}
      {#each visibleRows as row, i (getKey(row, startIdx + i))}
        <tr class="hover:bg-gray-800/60 border-b border-gray-800/50 transition-colors">
          {#each columns as col}
            <td
              class="px-3 py-2 text-gray-300 font-mono text-xs truncate max-w-xs"
              class:text-right={col.right}
            >
              <slot name="cell" {row} {col}>
                {(row as Record<string, unknown>)[col.key] ?? '—'}
              </slot>
            </td>
          {/each}
        </tr>
      {/each}
      {#if offsetBot > 0}
        <tr style="height:{offsetBot}px"><td colspan={columns.length} /></tr>
      {/if}
    </tbody>
  </table>
  {#if rows.length === 0}
    <div class="py-12 text-center text-gray-500 text-sm">No data</div>
  {/if}
</div>

<div class="mt-1 text-right text-xs text-gray-600">
  {rows.length.toLocaleString()} rows
</div>
