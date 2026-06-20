<script lang="ts">
  import { page } from '$app/stores';
  import { Activity, Network, Route, Bell, BarChart3, Shield, GitBranch, Cpu, Radio, BarChart2, Server, Filter, Zap, Map, TrendingUp } from 'lucide-svelte';

  const navItems = [
    { href: '/',           label: 'Dashboard',    icon: Activity },
    { href: '/peers',      label: 'Peers',        icon: Network },
    { href: '/prefixes',   label: 'Prefixes',     icon: Route },
    { href: '/topology',   label: 'Topology',     icon: BarChart3 },
    { href: '/alerts',     label: 'Alerts',       icon: Bell },
    { href: '/rpki',       label: 'RPKI',         icon: Shield },
    { href: '/rpki-coverage', label: 'RPKI Coverage', icon: TrendingUp },
    { href: '/policy',     label: 'Policy',       icon: GitBranch },
    { href: '/aspath',     label: 'AS Paths',     icon: Radio },
    { href: '/srpolicy',   label: 'SR Policy',    icon: Zap },
    { href: '/bgpls-path', label: 'BGP-LS Path',  icon: Map },
    { href: '/filters',    label: 'Filters',      icon: Filter },
    { href: '/path-status', label: 'Path Status',  icon: GitBranch },
    { href: '/capacity',   label: 'Capacity',      icon: TrendingUp },
    { href: '/onboard',    label: 'Onboarding',   icon: Server },
    { href: '/ml',         label: 'ML Insights',  icon: Cpu },
    { href: '/stats',      label: 'BMP Stats',    icon: BarChart2 },
  ];
</script>

<div class="flex h-screen overflow-hidden">
  <!-- Sidebar -->
  <aside class="w-56 flex-shrink-0 bg-gray-900 border-r border-gray-800 flex flex-col">
    <div class="px-4 py-5 border-b border-gray-800">
      <span class="text-lg font-bold text-emerald-400 tracking-tight">RustyBMP</span>
      <span class="ml-2 text-xs bg-emerald-800/60 text-emerald-300 px-1.5 py-0.5 rounded font-mono">RV6</span>
    </div>
    <nav class="flex-1 py-4 space-y-1 px-2">
      {#each navItems as item}
        {@const active = $page.url.pathname === item.href}
        <a
          href={item.href}
          class="flex items-center gap-3 px-3 py-2 rounded-lg text-sm transition-colors
                 {active
                   ? 'bg-emerald-600/20 text-emerald-400'
                   : 'text-gray-400 hover:text-gray-100 hover:bg-gray-800'}"
        >
          <svelte:component this={item.icon} size={16} />
          {item.label}
        </a>
      {/each}
    </nav>
    <div class="px-4 py-3 border-t border-gray-800 text-xs text-gray-600">
      rustybmp v0.1.0
    </div>
  </aside>

  <!-- Main content -->
  <main class="flex-1 overflow-auto bg-gray-950">
    <slot />
  </main>
</div>

<style>
  :global(body) { margin: 0; font-family: system-ui, sans-serif; }
  :global(*) { box-sizing: border-box; }
</style>
