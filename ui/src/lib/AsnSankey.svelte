<script lang="ts">
  /**
   * RV6-3: AsnSankey — D3 Sankey / flow diagram for AS-path visualisation.
   * Props:
   *   nodes  — [{ id: string, label: string }]
   *   links  — [{ source: string, target: string, value: number }]
   *   height — chart height in px (default 400)
   */
  import { onMount } from 'svelte';
  import * as d3 from 'd3';
  import { sankey, sankeyLinkHorizontal } from 'd3-sankey';

  export let nodes:  { id: string; label: string }[] = [];
  export let links:  { source: string; target: string; value: number }[] = [];
  export let height: number = 400;

  let container: HTMLDivElement;

  function draw() {
    if (!container || nodes.length === 0) return;
    const { width } = container.getBoundingClientRect();
    const margin = { top: 10, right: 10, bottom: 10, left: 10 };
    const W = width  - margin.left - margin.right;
    const H = height - margin.top  - margin.bottom;

    container.innerHTML = '';

    const svgEl = d3.select(container)
      .append('svg')
      .attr('width', '100%')
      .attr('height', height)
      .attr('viewBox', `0 0 ${width} ${height}`);

    const g = svgEl.append('g').attr('transform', `translate(${margin.left},${margin.top})`);

    // Build index map
    const nodeIndex = new Map(nodes.map((n, i) => [n.id, i]));
    const sankeyNodes = nodes.map((n, i) => ({ ...n, _idx: i }));
    const sankeyLinks = links
      .filter(l => nodeIndex.has(l.source) && nodeIndex.has(l.target))
      .map(l => ({
        source: nodeIndex.get(l.source)!,
        target: nodeIndex.get(l.target)!,
        value:  Math.max(1, l.value),
      }));

    if (sankeyLinks.length === 0) {
      g.append('text').attr('x', W / 2).attr('y', H / 2)
        .attr('text-anchor', 'middle').attr('fill', '#6b7280').attr('font-size', 13)
        .text('No AS path flow data');
      return;
    }

    const sankeyData = { nodes: sankeyNodes, links: sankeyLinks };

    const sk = (sankey as any)()
      .nodeId((_d: any, i: number) => i)
      .nodeWidth(16)
      .nodePadding(10)
      .extent([[0, 0], [W, H]]);

    const { nodes: snNodes, links: snLinks } = sk(sankeyData);

    const color = d3.scaleOrdinal(d3.schemeTableau10);

    // Links
    g.append('g')
      .selectAll('path')
      .data(snLinks)
      .join('path')
        .attr('d', sankeyLinkHorizontal() as any)
        .attr('fill', 'none')
        .attr('stroke', (d: any) => color(String(d.source.index)))
        .attr('stroke-width', (d: any) => Math.max(1, d.width))
        .attr('stroke-opacity', 0.35);

    // Nodes
    g.append('g')
      .selectAll('rect')
      .data(snNodes)
      .join('rect')
        .attr('x',      (d: any) => d.x0)
        .attr('y',      (d: any) => d.y0)
        .attr('width',  (d: any) => d.x1 - d.x0)
        .attr('height', (d: any) => Math.max(1, d.y1 - d.y0))
        .attr('fill',   (_: any, i: number) => color(String(i)))
        .attr('rx', 2);

    // Labels
    g.append('g')
      .selectAll('text')
      .data(snNodes)
      .join('text')
        .attr('x', (d: any) => d.x0 < W / 2 ? d.x1 + 6 : d.x0 - 6)
        .attr('y', (d: any) => (d.y1 + d.y0) / 2)
        .attr('dy', '0.35em')
        .attr('text-anchor', (d: any) => d.x0 < W / 2 ? 'start' : 'end')
        .attr('fill', '#d1d5db')
        .attr('font-size', 11)
        .text((d: any) => d.label ?? d.id);
  }

  onMount(() => { draw(); });
  $: nodes, links, draw();
</script>

<div bind:this={container} class="w-full" style="height:{height}px" />
