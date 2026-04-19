<!--
  @component
  TitleBarMenu — hamburger popover that replaces the old AppMenuBar.
  Groups File / View / Project items using DropdownMenuSeparator.
-->

<script lang="ts">
  import { iconButtonVariants } from '$lib/components/ui/button'
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuSeparator,
    DropdownMenuSub,
    DropdownMenuSubContent,
    DropdownMenuSubTrigger,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import { MenuIcon } from '$lib/icons/lucideExports'
  import { getProjects } from '$lib/stores/projects.svelte'
  import { isSidebarCollapsed, toggleSidebar, zoomIn, zoomOut, zoomReset } from '$lib/stores/ui.svelte'
  import { closeProject, getActiveProjectId, getOpenProjectIds, openProject } from '$lib/stores/workspace.svelte'

  let {
    onNewProject,
    onAbout
  }: {
    onNewProject: () => void
    onAbout?: () => void
  } = $props()

  let projects = $derived(getProjects())
  let openIds = $derived(getOpenProjectIds())
  let activeId = $derived(getActiveProjectId())
  let recentProjects = $derived(projects.filter((p) => !openIds.includes(p.id)))
  let hasActiveProject = $derived(activeId !== null)
  let sidebarCollapsed = $derived(isSidebarCollapsed())

  function handleCloseProject() {
    if (activeId) void closeProject(activeId)
  }
</script>

<DropdownMenuRoot>
  <DropdownMenuTrigger aria-label="Menu" class={iconButtonVariants({ size: 'md' })}>
    <MenuIcon size={14} />
  </DropdownMenuTrigger>

  <DropdownMenuContent align="end" sideOffset={6}>
    <DropdownMenuItem onclick={onNewProject}>Open Project</DropdownMenuItem>

    {#if recentProjects.length > 0}
      <DropdownMenuSub>
        <DropdownMenuSubTrigger>
          <span>Open Recent</span>
        </DropdownMenuSubTrigger>
        <DropdownMenuSubContent>
          {#each recentProjects as project (project.id)}
            <DropdownMenuItem onclick={() => openProject(project.id)}>
              <span class="truncate" title={project.path}>{project.name}</span>
            </DropdownMenuItem>
          {/each}
        </DropdownMenuSubContent>
      </DropdownMenuSub>
    {/if}

    {#if onAbout}
      <DropdownMenuItem onclick={onAbout}>About</DropdownMenuItem>
    {/if}

    <DropdownMenuItem onclick={handleCloseProject} disabled={!hasActiveProject}>Close Project</DropdownMenuItem>

    <DropdownMenuSeparator />

    <DropdownMenuItem onclick={toggleSidebar} disabled={!hasActiveProject}>
      {sidebarCollapsed ? 'Show' : 'Hide'} Sidebar
    </DropdownMenuItem>
    <DropdownMenuItem onclick={zoomIn}>Zoom In</DropdownMenuItem>
    <DropdownMenuItem onclick={zoomOut}>Zoom Out</DropdownMenuItem>
    <DropdownMenuItem onclick={zoomReset}>Reset Zoom</DropdownMenuItem>

    <DropdownMenuSeparator />

    <DropdownMenuItem disabled>
      <span>Tasks</span>
      <span class="ml-auto text-2xs text-muted italic">coming soon</span>
    </DropdownMenuItem>
  </DropdownMenuContent>
</DropdownMenuRoot>
