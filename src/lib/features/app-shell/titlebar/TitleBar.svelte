<script lang="ts">
  import { CommandPill } from '$lib/components/ui/command-pill'
  import TitleBarMenu from './TitleBarMenu.svelte'
  import TitleTabStrip from './TitleTabStrip.svelte'
  import WindowControls from './WindowControls.svelte'
  import { setCommandPaletteOpen } from '$lib/features/command-palette/state.svelte'

  let { onSettings }: { onSettings: () => void } = $props()

  function openPalette() {
    setCommandPaletteOpen(true)
  }
</script>

<header class="flex min-h-9 shrink-0 items-center border-b border-edge bg-surface">
  <TitleBarMenu {onSettings} />

  <TitleTabStrip />

  <!-- Draggable spacer: the only window-drag surface on the bar. -->
  <div data-tauri-drag-region class="min-w-0 flex-1 self-stretch"></div>

  <CommandPill onclick={openPalette} label="Search" class="w-56 shrink-0" />

  <WindowControls />
</header>
