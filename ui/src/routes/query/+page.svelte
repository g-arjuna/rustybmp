<script lang="ts">
  import { Search, Play, ChevronDown, ChevronRight, RefreshCw } from 'lucide-svelte';
  import { get, writable } from 'svelte/store';

  type ResultRow = Record<string, unknown>;

  const queryText = writable('');
  let loading    = $state(false);
  let error      = $state('');
  let sql        = $state('');
  let rows       = $state<ResultRow[]>([]);
  let columns    = $state<string[]>([]);
  let sqlOpen    = $state(false);
  let elapsed    = $state<number | null>(null);

  const EXAMPLES = [
    'Show RPKI invalid routes in the last hour',
    'Which peers have flapped more than 3 times today?',
    'What prefix has the longest AS path?',
    'Show top 10 origin ASNs by route count',
    'Which speakers have peers currently down?',
  ];

  async function runQuery() {
    const query = get(queryText).trim();
    if (!query) return;
    loading = true; error = ''; sql = ''; rows = []; columns = [];
    const t0 = performance.now();
    try {
      const res = await fetch('/api/nl-query', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ query }),
      });
      const j = await res.json() as { sql?: string; rows?: ResultRow[]; error?: string };
      elapsed = performance.now() - t0;
      if (!res.ok || j.error) { error = j.error ?? `HTTP ${res.status}`; return; }
      sql     = j.sql    ?? '';
      rows    = j.rows   ?? [];
      columns = rows.length > 0 ? Object.keys(rows[0]) : [];
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function handleKey(e: KeyboardEvent) {
    if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) runQuery();
  }
</script>

<svelte:head><title>NL Query — RustyBMP</title></svelte:head>

<div data-testid="page-query" class="p-6 space-y-6 max-w-5xl">
  <div class="space-y-1">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <Search size={22} class="text-emerald-400" /> Natural Language Query
    </h1>
    <p class="text-sm text-gray-500">Ask questions about your BGP data in plain English. Press <kbd class="bg-gray-800 border border-gray-700 px-1 py-0.5 rounded text-xs">⌘ Enter</kbd> to run.</p>
  </div>

  <!-- Example chips -->
  <div class="flex flex-wrap gap-2">
    {#each EXAMPLES as ex}
      <label
        data-testid="query-example-chip"
        class="cursor-pointer"
      >
        <input
          type="radio"
          name="query-example"
          value={ex}
          bind:group={$queryText}
          class="sr-only"
        />
        <span
        class="px-3 py-1.5 rounded-full text-xs bg-gray-800 border border-gray-700 text-gray-400
               hover:bg-gray-700 hover:text-gray-200 hover:border-emerald-600 transition-colors"
        >{ex}</span>
      </label>
    {/each}
  </div>

  <!-- Query input -->
  <div class="space-y-2">
    <textarea
      data-testid="query-input"
      bind:value={$queryText}
      onkeydown={handleKey}
      placeholder="e.g. Show me all RPKI invalid routes announced in the last 24 hours"
      rows="3"
      class="w-full bg-gray-900 border border-gray-700 rounded-xl px-4 py-3 text-sm text-gray-200
             placeholder-gray-600 focus:outline-none focus:border-emerald-500 resize-none font-mono"
    ></textarea>
    <div class="flex justify-end">
      <button
        data-testid="query-run-btn"
        onclick={runQuery}
        disabled={loading || !$queryText.trim()}
        class="flex items-center gap-2 px-5 py-2 bg-emerald-600 hover:bg-emerald-500 disabled:opacity-40
               disabled:cursor-not-allowed text-white text-sm rounded-lg font-medium transition-colors"
      >
        {#if loading}
          <RefreshCw size={14} class="animate-spin" /> Running…
        {:else}
          <Play size={14} /> Run Query
        {/if}
      </button>
    </div>
  </div>

  <!-- Error -->
  {#if error}
    <div data-testid="query-error" class="bg-red-900/30 border border-red-700 text-red-300 rounded-lg p-4 text-sm font-mono">{error}</div>
  {/if}

  <!-- Generated SQL (collapsible) -->
  {#if sql}
    <div class="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
      <button
        data-testid="query-sql-toggle"
        onclick={() => { sqlOpen = !sqlOpen; }}
        class="w-full flex items-center justify-between px-4 py-3 text-sm text-gray-400 hover:bg-gray-800 transition-colors"
      >
        <span class="flex items-center gap-2">
          {#if sqlOpen}<ChevronDown size={14} />{:else}<ChevronRight size={14} />{/if}
          Generated SQL
        </span>
        {#if elapsed !== null}
          <span class="text-xs text-gray-600">{elapsed.toFixed(0)} ms</span>
        {/if}
      </button>
      {#if sqlOpen}
        <pre data-testid="query-sql-display" class="px-4 pb-4 text-xs text-emerald-300 font-mono whitespace-pre-wrap overflow-x-auto">{sql}</pre>
      {/if}
    </div>
  {/if}

  <!-- Results table -->
  {#if rows.length > 0}
    <div class="space-y-2">
      <div class="flex items-center justify-between">
        <span data-testid="query-result-count" class="text-sm text-gray-400">{rows.length} row{rows.length === 1 ? '' : 's'} returned</span>
      </div>
      <div class="bg-gray-900 border border-gray-800 rounded-xl overflow-auto">
        <table data-testid="query-results-table" class="w-full text-sm min-w-max">
          <thead>
            <tr class="border-b border-gray-800 text-gray-500 text-xs uppercase tracking-wider">
              {#each columns as col}
                <th class="px-4 py-3 text-left">{col}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each rows as row, i}
              <tr data-testid="query-result-row-{i}" class="border-b border-gray-800/50 hover:bg-gray-800/30 transition-colors">
                {#each columns as col}
                  <td class="px-4 py-3 font-mono text-gray-300 text-xs max-w-xs truncate"
                      title={String(row[col] ?? '')}>
                    {row[col] ?? '—'}
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {:else if !loading && !error && sql}
    <p data-testid="query-no-results" class="text-sm text-gray-600 italic">No rows returned.</p>
  {/if}
</div>
