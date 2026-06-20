<script lang="ts">
  import { onMount } from 'svelte';
  import * as d3 from 'd3';
  import { api, type TopologyGraph } from '$lib/api';
  import { RefreshCw } from 'lucide-svelte';

  let svgEl: SVGSVGElement;
  let graph   = $state<TopologyGraph>({ nodes: [], links: [] });
  let loading = $state(true);
  let protocol = $state('');

  async function load() {
    loading = true;
    graph = await api.bgplsGraph(protocol || undefined).catch(() => ({ nodes: [], links: [] }));
    loading = false;
    draw();
  }

  function draw() {
    if (!svgEl) return;
    const el  = svgEl;
    const W   = el.clientWidth  || 900;
    const H   = el.clientHeight || 600;
    d3.select(el).selectAll('*').remove();

    const svg  = d3.select(el);
    const g    = svg.append('g');

    // Zoom + pan
    svg.call(
      d3.zoom<SVGSVGElement, unknown>()
        .scaleExtent([0.1, 8])
        .on('zoom', (e) => g.attr('transform', e.transform))
    );

    // Arrow marker
    svg.append('defs').append('marker')
      .attr('id', 'arrow')
      .attr('viewBox', '0 -4 8 8')
      .attr('refX', 18).attr('refY', 0)
      .attr('markerWidth', 6).attr('markerHeight', 6)
      .attr('orient', 'auto')
      .append('path').attr('d', 'M0,-4L8,0L0,4').attr('fill', '#4ade80');

    type N = { id: string; name: string | null; protocol: string | null; x?: number; y?: number; vx?: number; vy?: number; fx?: number | null; fy?: number | null };
    type L = { source: string | N; target: string | N; igp_metric: number | null };

    const nodes: N[] = graph.nodes.map(n => ({ ...n }));
    const links: L[] = graph.links.map(l => ({ ...l }));

    const simulation = d3.forceSimulation<N>(nodes)
      .force('link',   d3.forceLink<N, L>(links).id(d => d.id).distance(80))
      .force('charge', d3.forceManyBody().strength(-200))
      .force('center', d3.forceCenter(W / 2, H / 2))
      .force('collide', d3.forceCollide(20));

    // Links
    const link = g.append('g')
      .selectAll('line')
      .data(links)
      .join('line')
      .attr('stroke', '#334155')
      .attr('stroke-width', 1.5)
      .attr('marker-end', 'url(#arrow)');

    // Link labels (igp_metric)
    const linkLabel = g.append('g')
      .selectAll('text')
      .data(links.filter(l => l.igp_metric != null))
      .join('text')
      .attr('fill', '#64748b')
      .attr('font-size', '9px')
      .text(l => String(l.igp_metric));

    // Nodes
    const node = g.append('g')
      .selectAll<SVGCircleElement, N>('circle')
      .data(nodes)
      .join('circle')
      .attr('r', 10)
      .attr('fill', n => n.protocol === 'IsIsLevel2' ? '#6366f1' : '#10b981')
      .attr('stroke', '#1e293b')
      .attr('stroke-width', 2)
      .call(
        d3.drag<SVGCircleElement, N>()
          .on('start', (e, d) => {
            if (!e.active) simulation.alphaTarget(0.3).restart();
            d.fx = d.x; d.fy = d.y;
          })
          .on('drag', (e, d) => { d.fx = e.x; d.fy = e.y; })
          .on('end', (e, d) => {
            if (!e.active) simulation.alphaTarget(0);
            d.fx = null; d.fy = null;
          })
      );

    // Node labels
    const label = g.append('g')
      .selectAll('text')
      .data(nodes)
      .join('text')
      .attr('dy', -14)
      .attr('text-anchor', 'middle')
      .attr('fill', '#94a3b8')
      .attr('font-size', '10px')
      .text(n => n.name ?? n.id);

    simulation.on('tick', () => {
      link
        .attr('x1', d => (d.source as N).x ?? 0)
        .attr('y1', d => (d.source as N).y ?? 0)
        .attr('x2', d => (d.target as N).x ?? 0)
        .attr('y2', d => (d.target as N).y ?? 0);

      linkLabel
        .attr('x', d => (((d.source as N).x ?? 0) + ((d.target as N).x ?? 0)) / 2)
        .attr('y', d => (((d.source as N).y ?? 0) + ((d.target as N).y ?? 0)) / 2);

      node.attr('cx', d => d.x ?? 0).attr('cy', d => d.y ?? 0);
      label.attr('x', d => d.x ?? 0).attr('y', d => d.y ?? 0);
    });
  }

  onMount(() => { load(); });
</script>

<div class="p-6 space-y-4 h-full flex flex-col">
  <div class="flex items-center justify-between flex-wrap gap-3">
    <h1 class="text-2xl font-bold text-gray-100">BGP-LS Topology</h1>
    <div class="flex items-center gap-2">
      <select
        bind:value={protocol}
        onchange={load}
        class="bg-gray-900 border border-gray-700 rounded-lg px-3 py-1.5 text-sm text-gray-200
               focus:outline-none focus:border-emerald-500"
      >
        <option value="">All protocols</option>
        <option value="IsIsLevel1">IS-IS L1</option>
        <option value="IsIsLevel2">IS-IS L2</option>
        <option value="Ospfv2">OSPFv2</option>
        <option value="Ospfv3">OSPFv3</option>
        <option value="Direct">Direct</option>
      </select>
      <button
        onclick={load}
        class="p-2 rounded-lg bg-gray-800 hover:bg-gray-700 text-gray-400 hover:text-gray-100"
      >
        <RefreshCw size={14} />
      </button>
    </div>
  </div>

  <div class="flex gap-4 text-xs text-gray-500">
    <span class="flex items-center gap-1.5">
      <span class="inline-block w-3 h-3 rounded-full bg-indigo-500"></span> IS-IS L2
    </span>
    <span class="flex items-center gap-1.5">
      <span class="inline-block w-3 h-3 rounded-full bg-emerald-500"></span> Other
    </span>
    <span class="text-gray-600">
      {graph.nodes.length} nodes · {graph.links.length} links
    </span>
  </div>

  {#if loading}
    <p class="text-gray-500 text-sm">Loading topology…</p>
  {:else if graph.nodes.length === 0}
    <div class="flex-1 flex items-center justify-center text-gray-600 text-sm">
      No BGP-LS topology data available yet.
      <br />Requires BGP-LS (AFI=16388 SAFI=71) peering with a router.
    </div>
  {:else}
    <svg
      bind:this={svgEl}
      class="flex-1 bg-gray-900 rounded-xl border border-gray-800 w-full"
      style="min-height: 500px"
    ></svg>
  {/if}
</div>
