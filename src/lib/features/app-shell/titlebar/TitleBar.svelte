<script lang="ts">
  import { CommandPill } from '$lib/components/ui/command-pill'
  import { IconButton } from '$lib/components/ui/button'
  import TitleBarMenu from './TitleBarMenu.svelte'
  import TitleTabStrip from './TitleTabStrip.svelte'
  import WindowControls from './WindowControls.svelte'
  import { setCommandPaletteOpen } from '$lib/features/command-palette/state.svelte'
  import { SettingsIcon } from '$lib/icons/lucideExports'

  let { onSettings }: { onSettings: () => void } = $props()

  function openPalette() {
    setCommandPaletteOpen(true)
  }
</script>

<header class="flex min-h-9 shrink-0 items-center border-b border-edge bg-surface">
  <div class="flex shrink-0 items-center gap-0.5 self-stretch border-r border-edge px-1">
    <TitleBarMenu />
    <IconButton size="md" tooltip="Settings" onclick={onSettings}>
      <SettingsIcon size={14} />
    </IconButton>
  </div>

  <TitleTabStrip />

  <CommandPill onclick={openPalette} class="mr-1 w-60 shrink-0" />

  <WindowControls />
</header>
