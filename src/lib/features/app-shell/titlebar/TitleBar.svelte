<script lang="ts">
  import { CommandPill } from '$lib/components/ui/command-pill'
  import ProjectTabBar from './ProjectTabBar.svelte'
  import TitleBarMenu from './TitleBarMenu.svelte'
  import WindowControls from './WindowControls.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { Separator } from '$lib/components/ui/separator'
  import { SettingsIcon, Worm } from '$lib/icons/lucideExports'
  import { setCommandPaletteOpen } from '$lib/features/command-palette/state.svelte'

  let {
    onNewProject,
    onSettings,
    onAbout
  }: {
    onNewProject: () => void
    onSettings: () => void
    onAbout?: () => void
  } = $props()

  function openPalette() {
    setCommandPaletteOpen(true)
  }
</script>

<header data-tauri-drag-region class="flex min-h-9 shrink-0 items-center border-b border-edge bg-surface">
  <!-- Logo (static branding) — width matches SidebarRail so icons align vertically -->
  <div class="flex h-8 w-9 shrink-0 items-center justify-center text-accent" aria-hidden="true">
    <Worm size={18} />
  </div>

  <div class="min-w-0 flex-1 self-stretch">
    <ProjectTabBar onAddProject={onNewProject} />
  </div>

  <Separator orientation="vertical" class="mx-1.5 h-4" />

  <div class="flex items-center gap-1">
    <CommandPill onclick={openPalette} />

    <IconButton size="md" tooltip="Settings" onclick={onSettings}>
      <SettingsIcon size={14} />
    </IconButton>

    <TitleBarMenu {onNewProject} {onAbout} />
  </div>

  <WindowControls />
</header>
