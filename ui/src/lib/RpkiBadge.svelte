<script lang="ts">
  /** RV6-3: RpkiBadge — colour-coded RPKI validity pill. */
  export let validity: string | null | undefined = 'unknown';

  type BadgeCfg = { label: string; cls: string };

  const cfg: Record<string, BadgeCfg> = {
    valid:     { label: 'VALID',     cls: 'bg-green-900/60 text-green-300 border-green-600/50' },
    invalid:   { label: 'INVALID',   cls: 'bg-red-900/60 text-red-300 border-red-600/50' },
    'not-found': { label: 'NOT FOUND', cls: 'bg-yellow-900/60 text-yellow-300 border-yellow-600/50' },
    unknown:   { label: 'UNKNOWN',   cls: 'bg-gray-800/60 text-gray-400 border-gray-600/50' },
  };

  $: v   = (validity ?? 'unknown').toLowerCase();
  $: cur = cfg[v] ?? cfg.unknown;
</script>

<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-semibold border tracking-wide {cur.cls}">
  {cur.label}
</span>
