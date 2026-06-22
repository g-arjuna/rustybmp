<script lang="ts">
  import { onMount } from 'svelte';
  import { Tag, RefreshCw, TrendingUp, Info } from 'lucide-svelte';

  interface CommunityFreq {
    community:     string;
    route_count:   number;
    pre_policy:    number;
    post_policy:   number;
    first_seen:    string | null;
    last_changed:  string | null;
  }

  interface SemanticLabel {
    community:   string;
    meaning:     string;
    confidence:  number;
    pattern:     string | null;
  }

  let freqs     = $state<CommunityFreq[]>([]);
  let semantics = $state<SemanticLabel[]>([]);
  let loading   = $state(true);
  let search    = $state('');

  async function load() {
    loading = true;
    try {
      const [fr, sr] = await Promise.allSettled([
        fetch('/api/communities').then(r => r.json() as Promise<{ communities: CommunityFreq[] }>),
        fetch('/api/communities/semantics').then(r => r.json() as Promise<{ semantics: SemanticLabel[] }>),
      ]);
      freqs     = fr.status === 'fulfilled' ? fr.value.communities ?? [] : [];
      semantics = sr.status === 'fulfilled' ? sr.value.semantics   ?? [] : [];
    } finally {
      loading = false;
    }
  }

  onMount(load);

  const semanticMap = $derived(
    new Map(semantics.map(s => [s.community, s]))
  );

  const filtered = $derived(
    search
      ? freqs.filter(c =>
          c.community.includes(search) ||
          (semanticMap.get(c.community)?.meaning ?? '').toLowerCase().includes(search.toLowerCase())
        )
      : freqs
  );

  const maxCount = $derived(filtered.reduce((m, c) => Math.max(m, c.route_count), 1));

  function filterDelta(c: CommunityFreq) {
    if (c.pre_policy === 0) return null;
    return c.post_policy - c.pre_policy;
  }
</script>

<svelte:head><title>Communities Explorer — RustyBMP</title></svelte:head>

<div data-testid="page-communities" class="p-6 space-y-6">
  <div class="flex items-center justify-between flex-wrap gap-3">
    <h1 class="text-2xl font-bold text-gray-100 flex items-center gap-2">
      <Tag size={22} class="text-yellow-400" /> Communities Explorer
    </h1>
    <div class="flex items-center gap-3">
      <input
        data-testid="communities-search"
        bind:value={search}
        placeholder="Filter communities…"
        class="bg-gray-900 border border-gray-700 rounded-lg px-3 py-1.5 text-sm text-gray-200
               placeholder-gray-600 focus:outline-none focus:border-emerald-500 w-52 font-mono"
      />
      <button
        data-testid="communities-refresh"
        onclick={load}
        class="p-2 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-gray-100 transition-colors"
        title="Refresh"
      >
        <RefreshCw size={15} />
      </button>
    </div>
  </div>

  <!-- Semantics summary banner -->
  {#if semantics.length > 0}
    <div data-testid="communities-semantics-banner"
         class="bg-yellow-900/15 border border-yellow-700/40 rounded-xl p-4 flex items-start gap-3">
      <Info size={16} class="text-yellow-400 mt-0.5 flex-shrink-0" />
      <div>
        <p class="text-sm text-yellow-300 font-medium">{semantics.length} community meanings learned via fpgrowth mining</p>
        <p class="text-xs text-yellow-600 mt-0.5">
          Inferred from pre/post-policy pattern correlations across all peers. Confidence ≥ 0.7 shown.
        </p>
      </div>
    </div>
  {/if}

  {#if loading}
    <p class="text-gray-500 text-sm animate-pulse">Loading communities…</p>
  {:else if filtered.length === 0}
    <div class="bg-gray-900 border border-gray-800 rounded-xl p-10 text-center text-gray-600 text-sm">
      No community data available yet.
    </div>
  {:else}
    <div class="bg-gray-900 border border-gray-800 rounded-xl overflow-hidden">
      <table data-testid="communities-table" class="w-full text-sm">
        <thead>
          <tr class="border-b border-gray-800 text-gray-500 text-xs uppercase tracking-wider">
            <th class="px-4 py-3 text-left">Community</th>
            <th class="px-4 py-3 text-left">Inferred Meaning</th>
            <th class="px-4 py-3 text-left">Frequency</th>
            <th class="px-4 py-3 text-right">Pre-policy</th>
            <th class="px-4 py-3 text-right">Post-policy</th>
            <th class="px-4 py-3 text-right">Filter Δ</th>
            <th class="px-4 py-3 text-right">First Seen</th>
          </tr>
        </thead>
        <tbody>
          {#each filtered as c}
            {@const sem   = semanticMap.get(c.community)}
            {@const delta = filterDelta(c)}
            {@const barPct = Math.round(c.route_count / maxCount * 100)}
            <tr data-testid="community-row-{c.community}"
                class="border-b border-gray-800/50 hover:bg-gray-800/30 transition-colors">
              <td class="px-4 py-3 font-mono text-emerald-300">{c.community}</td>
              <td class="px-4 py-3">
                {#if sem}
                  <span class="text-gray-200 text-xs">{sem.meaning}</span>
                  <span class="ml-1.5 text-xs text-gray-600">({Math.round(sem.confidence * 100)}%)</span>
                {:else}
                  <span class="text-gray-600 text-xs italic">unknown</span>
                {/if}
              </td>
              <td class="px-4 py-3 w-40">
                <div class="flex items-center gap-2">
                  <div class="flex-1 h-2 bg-gray-800 rounded-full overflow-hidden">
                    <div class="h-2 bg-yellow-500/70 rounded-full" style="width:{barPct}%"></div>
                  </div>
                  <span class="text-gray-400 text-xs w-12 text-right font-mono">{c.route_count.toLocaleString()}</span>
                </div>
              </td>
              <td class="px-4 py-3 text-right text-gray-400 font-mono text-xs">{c.pre_policy.toLocaleString()}</td>
              <td class="px-4 py-3 text-right text-gray-400 font-mono text-xs">{c.post_policy.toLocaleString()}</td>
              <td class="px-4 py-3 text-right font-mono text-xs">
                {#if delta === null}
                  <span class="text-gray-600">—</span>
                {:else if delta < 0}
                  <span class="text-red-400">{delta.toLocaleString()}</span>
                {:else if delta > 0}
                  <span class="text-emerald-400">+{delta.toLocaleString()}</span>
                {:else}
                  <span class="text-gray-500">0</span>
                {/if}
              </td>
              <td class="px-4 py-3 text-right text-gray-500 text-xs">
                {c.first_seen ? new Date(c.first_seen).toLocaleDateString() : '—'}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <p class="text-xs text-gray-600">{filtered.length} of {freqs.length} communities</p>
  {/if}
</div>
