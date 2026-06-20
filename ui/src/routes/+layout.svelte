<script lang="ts">
  import { page } from '$app/stores';
  import { Activity, Network, Route, Bell, BarChart3 } from 'lucide-svelte';

  const navItems = [
    { href: '/',          label: 'Dashboard',  icon: Activity },
    { href: '/peers',     label: 'Peers',      icon: Network },
    { href: '/prefixes',  label: 'Prefixes',   icon: Route },
    { href: '/topology',  label: 'Topology',   icon: BarChart3 },
    { href: '/alerts',    label: 'Alerts',     icon: Bell },
  ];
</script>

<div class="flex h-screen overflow-hidden">
  <!-- Sidebar -->
  <aside class="w-56 flex-shrink-0 bg-gray-900 border-r border-gray-800 flex flex-col">
    <div class="px-4 py-5 border-b border-gray-800">
      <span class="text-lg font-bold text-emerald-400 tracking-tight">RustyBMP</span>
      <span class="ml-2 text-xs text-gray-500">RV4</span>
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
