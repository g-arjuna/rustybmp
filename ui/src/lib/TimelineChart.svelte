<script lang="ts">
  /**
   * RV6-3: TimelineChart — D3-powered area/line chart for time-series data.
   * Props:
   *   data     — array of { t: string (ISO), v: number }
   *   label    — y-axis label
   *   color    — stroke colour (CSS)
   *   height   — px height (default 180)
   *   format   — d3 number format string for y-axis ticks
   */
  import { onMount } from 'svelte';
  import * as d3 from 'd3';

  export let data:    { t: string; v: number }[] = [];
  export let label:   string  = '';
  export let color:   string  = '#60a5fa';
  export let height:  number  = 180;
  export let format:  string  = ',.0f';

  let container: HTMLDivElement;

  function draw() {
    if (!container || data.length === 0) return;
    const { width } = container.getBoundingClientRect();
    const margin    = { top: 10, right: 10, bottom: 28, left: 44 };
    const W = width  - margin.left - margin.right;
    const H = height - margin.top  - margin.bottom;

    container.innerHTML = '';

    const svgEl = d3.select(container)
      .append('svg')
      .attr('width', '100%')
      .attr('height', height)
      .attr('viewBox', `0 0 ${width} ${height}`);

    const g = svgEl.append('g').attr('transform', `translate(${margin.left},${margin.top})`);

    const parsed = data.map(d => ({ t: new Date(d.t), v: d.v }));

    const xScale = d3.scaleTime()
      .domain(d3.extent(parsed, d => d.t) as [Date, Date])
      .range([0, W]);
    const yScale = d3.scaleLinear()
      .domain([0, d3.max(parsed, d => d.v) ?? 1])
      .nice()
      .range([H, 0]);

    // Grid lines
    g.append('g')
      .attr('class', 'grid')
      .call(d3.axisLeft(yScale).ticks(4).tickSize(-W).tickFormat(() => ''))
      .selectAll('line')
      .attr('stroke', '#374151')
      .attr('stroke-dasharray', '3,3');
    g.select('.grid .domain').remove();

    // Area
    const area = d3.area<{ t: Date; v: number }>()
      .x(d => xScale(d.t))
      .y0(H)
      .y1(d => yScale(d.v))
      .curve(d3.curveMonotoneX);

    g.append('path')
      .datum(parsed)
      .attr('fill', color)
      .attr('fill-opacity', 0.12)
      .attr('d', area);

    // Line
    const line = d3.line<{ t: Date; v: number }>()
      .x(d => xScale(d.t))
      .y(d => yScale(d.v))
      .curve(d3.curveMonotoneX);

    g.append('path')
      .datum(parsed)
      .attr('fill', 'none')
      .attr('stroke', color)
      .attr('stroke-width', 1.5)
      .attr('d', line);

    // X axis
    g.append('g')
      .attr('transform', `translate(0,${H})`)
      .call(d3.axisBottom(xScale).ticks(5).tickFormat(d3.timeFormat('%H:%M') as (d: Date | d3.NumberValue) => string))
      .selectAll('text, line, path')
      .attr('stroke', '#6b7280')
      .attr('fill', '#6b7280');

    // Y axis
    g.append('g')
      .call(d3.axisLeft(yScale).ticks(4).tickFormat(d3.format(format) as (d: d3.NumberValue) => string))
      .selectAll('text, line, path')
      .attr('stroke', '#6b7280')
      .attr('fill', '#6b7280');

    if (label) {
      g.append('text')
        .attr('transform', 'rotate(-90)')
        .attr('x', -H / 2)
        .attr('y', -36)
        .attr('text-anchor', 'middle')
        .attr('fill', '#9ca3af')
        .attr('font-size', 10)
        .text(label);
    }
  }

  onMount(() => { draw(); });
  $: data, draw();
</script>

<div bind:this={container} class="w-full" style="height:{height}px" />
