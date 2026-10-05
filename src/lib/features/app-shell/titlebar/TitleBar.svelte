<script lang="ts">
  import { platform } from '$lib/platform'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { IconButton } from '$lib/components/ui/button'
  import TitleBarMenu from './TitleBarMenu.svelte'
  import TitleTabStrip from './TitleTabStrip.svelte'
  import WindowControls from './WindowControls.svelte'
  import { setCommandPaletteOpen } from '$lib/features/command-palette/state.svelte'
  import { CompassIcon } from '$lib/icons/lucideExports'
  import { isBrowserOpen, toggleBrowser } from '$lib/features/browser/state.svelte'
  import { getCommandShortcut } from '$lib/features/command-palette/shortcuts/registry.svelte'
  import { splitShortcut } from '$lib/features/command-palette/shortcuts/spec'

  let browserShortcut = $derived(getCommandShortcut('open-folder'))
  let paletteShortcut = $derived(splitShortcut(getCommandShortcut('toggle-command-palette')))

  function openPalette() {
    setCommandPaletteOpen(true)
  }
</script>

<header class="flex min-h-9 shrink-0 items-center border-b border-edge bg-surface">
  <div class="flex shrink-0 items-center gap-0.5 self-stretch border-r border-edge px-1">
    <TitleBarMenu />
    <IconButton
      size="md"
      tooltip="Open Folder"
      shortcut={browserShortcut}
      aria-expanded={isBrowserOpen()}
      onclick={() => toggleBrowser()}
    >
      <CompassIcon size={14} />
    </IconButton>
  </div>

  <TitleTabStrip />

  <button
    data-slot="command-pill"
    type="button"
    class="mr-1 inline-flex h-[26px] w-60 shrink-0 cursor-pointer items-center gap-2.5 rounded-md border border-edge bg-raised pr-2 pl-2.5 text-xs text-muted transition-colors hover:border-accent hover:text-bright focus-visible:shadow-focus-ring focus-visible:outline-none"
    onclick={openPalette}
  >
    <span class="truncate">Commands…</span>
    <KbdGroup class="ml-auto">
      {#each paletteShortcut as part (part)}
        <Kbd class="rounded-sm">{part}</Kbd>
      {/each}
    </KbdGroup>
  </button>

  {#if platform.native}
    <WindowControls />
  {/if}
</header>
