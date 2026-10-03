<script lang="ts">
  import { platform } from '$lib/platform'
  import { CommandPill } from '$lib/components/ui/command-pill'
  import { IconButton } from '$lib/components/ui/button'
  import TitleBarMenu from './TitleBarMenu.svelte'
  import TitleTabStrip from './TitleTabStrip.svelte'
  import WindowControls from './WindowControls.svelte'
  import { setCommandPaletteOpen } from '$lib/features/command-palette/state.svelte'
  import { CompassIcon } from '$lib/icons/lucideExports'
  import { isBrowserOpen, toggleBrowser } from '$lib/features/browser/state.svelte'
  import { getEffectiveSpec } from '$lib/features/command-palette/shortcuts/overrides.svelte'

  let placesShortcut = $derived(getEffectiveSpec('open-folder', 'Ctrl+O'))

  function openPalette() {
    setCommandPaletteOpen(true)
  }
</script>

<header class="flex min-h-9 shrink-0 items-center border-b border-edge bg-surface">
  <div class="flex shrink-0 items-center gap-0.5 self-stretch border-r border-edge px-1">
    <TitleBarMenu />
    <IconButton
      size="md"
      tooltip="Places"
      shortcut={placesShortcut}
      data-browser-toggle="true"
      aria-expanded={isBrowserOpen()}
      onclick={() => toggleBrowser()}
    >
      <CompassIcon size={14} />
    </IconButton>
  </div>

  <TitleTabStrip />

  <CommandPill onclick={openPalette} class="mr-1 w-60 shrink-0" />

  {#if platform.capabilities.nativeWindowControls}
    <WindowControls />
  {/if}
</header>
