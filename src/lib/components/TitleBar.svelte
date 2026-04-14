<script lang="ts">
  import AppMenuBar from '$lib/components/AppMenuBar.svelte'
  import ProjectTabBar from '$lib/components/ProjectTabBar.svelte'
  import WindowControls from '$lib/components/WindowControls.svelte'
  import { Worm } from '$lib/icons/lucideExports'

  let {
    onNewProject,
    onSettings,
    onAbout
  }: {
    onNewProject: () => void
    onSettings: () => void
    onAbout?: () => void
  } = $props()
</script>

<header class="flex min-h-9 shrink-0 items-center gap-1 border-b border-edge bg-surface pl-1" data-tauri-drag-region>
  <!-- Logo (static branding) -->
  <div class="flex h-8 w-8 shrink-0 items-center justify-center text-accent">
    <Worm size={18} />
  </div>

  <!-- Menubar -->
  <div class="shrink-0">
    <AppMenuBar {onNewProject} {onSettings} {onAbout} />
  </div>

  <div class="min-w-0 flex-1">
    <ProjectTabBar onAddProject={onNewProject} />
  </div>

  <WindowControls />
</header>
