<script lang="ts">
  import {
    MenubarContent,
    MenubarItem,
    MenubarMenu,
    MenubarRoot,
    MenubarSeparator,
    MenubarSub,
    MenubarSubContent,
    MenubarSubTrigger,
    MenubarTrigger
  } from '$lib/components/ui/menubar'
  import { getProjects } from '$lib/stores/projects.svelte'
  import { isGitSidebarCollapsed, toggleGitSidebar, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'
  import { closeProject, getActiveProjectId, getOpenProjectIds, openProject } from '$lib/stores/workspace.svelte'
  import { ChevronRight } from '$lib/icons/lucideExports'

  let {
    onNewProject,
    onSettings,
    onAbout
  }: {
    onNewProject: () => void
    onSettings: () => void
    onAbout?: () => void
  } = $props()

  let projects = $derived(getProjects())
  let openIds = $derived(getOpenProjectIds())
  let activeId = $derived(getActiveProjectId())
  let recentProjects = $derived(projects.filter((p) => !openIds.includes(p.id)))
  let hasActiveProject = $derived(activeId !== null)
  let sidebarCollapsed = $derived(isGitSidebarCollapsed())

  function handleCloseProject() {
    if (activeId) closeProject(activeId)
  }
</script>

<MenubarRoot class="flex items-center gap-0.5 rounded px-0.5 py-0.5">
  <!-- File menu -->
  <MenubarMenu>
    <MenubarTrigger>File</MenubarTrigger>
    <MenubarContent>
      <MenubarItem onclick={onNewProject}>
        Open Project
        <span class="text-[0.68rem] text-muted">Ctrl+O</span>
      </MenubarItem>

      {#if recentProjects.length > 0}
        <MenubarSub>
          <MenubarSubTrigger
            class="flex w-full cursor-pointer items-center justify-between rounded-sm px-3 py-1.5 text-left text-fg transition-colors outline-none hover:bg-surface focus:bg-surface"
          >
            Open Recent
            <ChevronRight size={12} class="text-muted" />
          </MenubarSubTrigger>
          <MenubarSubContent>
            {#each recentProjects as project (project.id)}
              <MenubarItem onclick={() => openProject(project.id)}>
                <span class="truncate" title={project.path}>{project.name}</span>
              </MenubarItem>
            {/each}
          </MenubarSubContent>
        </MenubarSub>
      {/if}

      <MenubarSeparator />
      <MenubarItem onclick={onSettings}>Settings</MenubarItem>
      {#if onAbout}
        <MenubarItem onclick={onAbout}>About</MenubarItem>
      {/if}
      <MenubarSeparator />
      <MenubarItem onclick={handleCloseProject} disabled={!hasActiveProject}>Close Project</MenubarItem>
    </MenubarContent>
  </MenubarMenu>

  <!-- View menu -->
  <MenubarMenu>
    <MenubarTrigger>View</MenubarTrigger>
    <MenubarContent>
      <MenubarItem onclick={toggleGitSidebar} disabled={!hasActiveProject}>
        {sidebarCollapsed ? 'Show' : 'Hide'} Git Sidebar
      </MenubarItem>
      <MenubarSeparator />
      <MenubarItem onclick={zoomIn}>
        Zoom In
        <span class="text-[0.68rem] text-muted">Ctrl+=</span>
      </MenubarItem>
      <MenubarItem onclick={zoomOut}>
        Zoom Out
        <span class="text-[0.68rem] text-muted">Ctrl+-</span>
      </MenubarItem>
      <MenubarItem onclick={zoomReset}>
        Reset Zoom
        <span class="text-[0.68rem] text-muted">Ctrl+0</span>
      </MenubarItem>
    </MenubarContent>
  </MenubarMenu>

  <!-- Project menu (context-dependent) -->
  <MenubarMenu>
    <MenubarTrigger>Project</MenubarTrigger>
    <MenubarContent>
      <MenubarItem disabled={true}>
        Tasks
        <span class="text-[0.68rem] text-muted italic">coming soon</span>
      </MenubarItem>
    </MenubarContent>
  </MenubarMenu>
</MenubarRoot>
