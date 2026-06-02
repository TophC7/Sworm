<script lang="ts">
  import { CommandPill } from '$lib/components/ui/command-pill'
  import AppMenuBar from './AppMenuBar.svelte'
  import TitleBarMenu from './TitleBarMenu.svelte'
  import WindowControls from './WindowControls.svelte'
  import { Worm } from '$lib/icons/lucideExports'
  import { setCommandPaletteOpen } from '$lib/features/command-palette/state.svelte'
  import { getActiveProject } from '$lib/features/projects/state.svelte'

  let activeProject = $derived(getActiveProject())

  let {
    onNewProject,
    onSettings
  }: {
    onNewProject: () => void
    onSettings: () => void
  } = $props()

  function openPalette() {
    setCommandPaletteOpen(true)
  }

  // RESPONSIVE MENU //
  // Show the horizontal menu bar while it fits in the gap between the
  // logo and the centered command pill; otherwise collapse to the
  // hamburger. The menu bar's trigger row is static, so its intrinsic
  // width is measured once (while shown) and reused for the fit test.
  const LOGO_WIDTH = 36 // w-9
  const PILL_FRACTION = 3 / 8 // CommandPill is w-3/8 of the bar
  const SAFETY = 12

  let headerEl = $state<HTMLElement | null>(null)
  let menuFits = $state(true)
  let menuWidth = 0

  function recomputeFit() {
    const w = headerEl?.clientWidth ?? 0
    if (!w) return
    const leftGap = ((1 - PILL_FRACTION) / 2) * w - LOGO_WIDTH - SAFETY
    const nextFits = menuWidth === 0 || menuWidth <= leftGap
    if (nextFits !== menuFits) menuFits = nextFits
  }

  // Capture the menu bar's natural width the first time it lays out.
  function measureMenu(el: HTMLElement) {
    if (menuWidth === 0) {
      menuWidth = el.offsetWidth
      recomputeFit()
    }
  }

  $effect(() => {
    const el = headerEl
    if (!el) return
    const observer = new ResizeObserver(() => recomputeFit())
    observer.observe(el)
    return () => observer.disconnect()
  })
</script>

<header
  bind:this={headerEl}
  data-tauri-drag-region
  class="relative flex min-h-9 shrink-0 items-center border-b border-edge bg-surface"
>
  <!-- Logo (static branding) — width matches SidebarRail so icons align vertically -->
  <div class="flex h-8 w-9 shrink-0 items-center justify-center text-accent" aria-hidden="true">
    <Worm size={18} />
  </div>

  <!-- Left menu slot: the full menu bar when it fits, the hamburger otherwise. -->
  <div class="flex shrink-0 items-center">
    {#if menuFits}
      <div {@attach measureMenu}>
        <AppMenuBar {onNewProject} {onSettings} />
      </div>
    {:else}
      <TitleBarMenu {onNewProject} {onSettings} />
    {/if}
  </div>

  <!-- Draggable spacer; keeps the right controls right-aligned while the
       command pill is centered over the whole bar (below). -->
  <div data-tauri-drag-region class="min-w-0 flex-1 self-stretch"></div>

  <WindowControls />

  <!-- Command pill pinned to the TRUE center of the bar, independent of
       the chrome around it. Width tracks the viewport. The wrapper is
       click-through so drags still land on the bar beside the pill. -->
  <div class="pointer-events-none absolute inset-x-0 top-1/2 flex -translate-y-1/2 justify-center">
    <CommandPill onclick={openPalette} label={activeProject?.name} class="pointer-events-auto w-3/8" />
  </div>
</header>
