<!--
  @component
  ProjectsSidebar. Cross-project navigation tree: every registered
  project, each expandable to its agent sessions nested underneath.
  Replaces both the top project tabs and the old per-project sessions
  view as the primary navigator.

  Shape mirrors IssuesSidebar (epic -> issue -> sub-issue): SidebarRow /
  sidebarRowVariants for rows, tree indent guides for nesting, context
  menus for actions, a Set for expand state. Reads global project and
  session state, so it ignores the active-project prop entirely.

  Click rules:
    - Click a project row     -> activate it + toggle its expansion.
    - Click a session row      -> open it as a tab in its project.
    - Right-click any row      -> context-menu actions.
-->

<script lang="ts">
  import { onMount, untrack } from 'svelte'
  import { revealItemInDir } from '@tauri-apps/plugin-opener'
  import { cn } from '$lib/utils/cn'
  import { SvelteSet } from 'svelte/reactivity'
  import SidebarPanel from '$lib/features/app-shell/sidebar/SidebarPanel.svelte'
  import { SidebarRow, sidebarRowVariants } from '$lib/components/ui/sidebar-row'
  import TabBeam from '$lib/components/ui/tab-beam.svelte'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { IconButton } from '$lib/components/ui/button'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import {
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuRoot,
    ContextMenuSeparator,
    ContextMenuTrigger
  } from '$lib/components/ui/context-menu'
  import {
    ArchiveIcon,
    ArchiveRestoreIcon,
    ArrowDownUpIcon,
    FolderOpen,
    Plus,
    SettingsIcon,
    Trash2
  } from '$lib/icons/lucideExports'
  import { getProjects } from '$lib/features/projects/state.svelte'
  import { removeProject } from '$lib/features/projects/state.svelte'
  import {
    getActiveProjectId,
    getActiveSessionId,
    openProject,
    closeProject,
    closeTabBySessionId,
    openLauncherTab
  } from '$lib/features/workbench/state.svelte'
  import {
    getAgentSessions,
    getArchivedAgentSessions,
    loadSessionGroupsForDisplay,
    archiveSession,
    unarchiveSession,
    removeSession
  } from '$lib/features/sessions/state/sessions.svelte'
  import { ensureSessionSurface } from '$lib/features/workbench/surfaces/session/service.svelte'
  import { sessionDotClass, SESSION_STATUS_LEGEND } from '$lib/features/sessions/visual'
  import { allProviders, directOptions } from '$lib/features/sessions/providers/catalog'
  import {
    isProjectExpanded,
    toggleProjectExpanded,
    getProjectSort,
    setProjectSort,
    loadProjectsNavState,
    type ProjectSort
  } from '$lib/features/app-shell/sidebar/state.svelte'
  import { openProjectDirectory, openProjectSettingsFile } from '$lib/features/app-actions/actions.svelte'
  import { confirmAsync } from '$lib/features/confirm/service.svelte'
  import { runNotifiedTask, getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { timeAgo } from '$lib/utils/date'
  import type { Project, Session } from '$lib/types/backend'

  // Provider glyph lookup; same source as DiscoveredProjectsPanel.
  const providerMap = new Map([...allProviders, ...directOptions].map((p) => [p.id, p]))

  // Collapsed projects show only their latest session; this caps how
  // many sessions an expanded project lists before a "show more" row.
  const SESSION_PREVIEW_LIMIT = 6

  let projects = $derived(getProjects())
  let activeProjectId = $derived(getActiveProjectId())
  let activeSessionId = $derived(getActiveSessionId())
  let sort = $derived(getProjectSort())

  let sortedProjects = $derived(sortProjects(projects, sort))
  // Archived division: only projects that actually have archived sessions.
  let archivedProjects = $derived(sortedProjects.filter((p) => getArchivedAgentSessions(p.id).length > 0))
  let archivedTotal = $derived(archivedProjects.reduce((sum, p) => sum + getArchivedAgentSessions(p.id).length, 0))

  // Per-project UI toggles, local to this surface. Live-tree expansion
  // persists via sidebar state; archived-tree expansion is ephemeral.
  const showAllSessions = new SvelteSet<string>()
  const expandedArchived = new SvelteSet<string>()

  onMount(() => {
    void loadProjectsNavState()
  })

  // Fill the display cache in one backend call so collapsed rows can
  // show latest-session summaries without per-project fanout. Re-runs
  // when a project is added or removed (the array identity changes).
  // untrack keeps the session-cache writes from retriggering this effect.
  $effect(() => {
    const ids = projects.map((p) => p.id)
    untrack(() => {
      void loadSessionGroupsForDisplay(ids)
    })
  })

  function latestSession(projectId: string): Session | null {
    return getAgentSessions(projectId)[0] ?? null
  }

  function activityTime(projectId: string): number {
    const latest = latestSession(projectId)
    return latest ? Date.parse(latest.updated_at) : 0
  }

  function sortProjects(list: Project[], mode: ProjectSort): Project[] {
    const copy = [...list]
    if (mode === 'name') copy.sort((a, b) => a.name.localeCompare(b.name))
    else copy.sort((a, b) => activityTime(b.id) - activityTime(a.id))
    return copy
  }

  function toggleSort() {
    setProjectSort(sort === 'recent' ? 'name' : 'recent')
  }

  function toggleShowAll(projectId: string) {
    if (showAllSessions.has(projectId)) showAllSessions.delete(projectId)
    else showAllSessions.add(projectId)
  }

  function toggleArchivedExpanded(projectId: string) {
    if (expandedArchived.has(projectId)) expandedArchived.delete(projectId)
    else expandedArchived.add(projectId)
  }

  // Single click toggles the row's expansion; double click opens the
  // project (mirrors the issues rows). dblclick fires two clicks first,
  // so the expansion nets back to where it started before opening.
  function toggleProjectRow(projectId: string, archived: boolean) {
    if (archived) toggleArchivedExpanded(projectId)
    else toggleProjectExpanded(projectId)
  }

  // PROJECT ACTIONS //
  function handleNewSession(project: Project) {
    openProject(project.id)
    openLauncherTab(project.id)
  }

  function handleReveal(project: Project) {
    void revealItemInDir(project.path).catch((error) =>
      notify.error('Reveal in file manager failed', getErrorMessage(error))
    )
  }

  async function handleProjectSettings(project: Project) {
    openProject(project.id)
    await openProjectSettingsFile()
  }

  async function handleRemoveProject(project: Project) {
    const ok = await confirmAsync({
      title: `Remove ${project.name}?`,
      message: 'Removes the project from Sworm. Files on disk are not deleted.',
      confirmLabel: 'Remove'
    })
    if (!ok) return
    await runNotifiedTask(() => removeProject(project.id), {
      loading: { title: 'Removing project', description: project.name },
      success: { title: 'Project removed', description: project.name },
      error: { title: 'Remove project failed' }
    })
  }

  // SESSION ACTIONS //
  function handleSessionClick(session: Session) {
    void ensureSessionSurface(session.project_id, session.id, session.title, session.provider_id)
  }

  async function handleArchive(session: Session) {
    const result = await runNotifiedTask(() => archiveSession(session.id, session.project_id), {
      loading: { title: 'Archiving session', description: session.id.slice(0, 8) },
      success: { title: 'Session archived', description: session.id.slice(0, 8) },
      error: { title: 'Archive session failed' }
    })
    if (result !== undefined) closeTabBySessionId(session.project_id, session.id)
  }

  async function handleUnarchive(session: Session) {
    await runNotifiedTask(() => unarchiveSession(session.id, session.project_id), {
      loading: { title: 'Restoring session', description: session.id.slice(0, 8) },
      success: { title: 'Session restored', description: session.id.slice(0, 8) },
      error: { title: 'Restore session failed' }
    })
  }

  async function handleDeleteSession(session: Session) {
    const ok = await confirmAsync({
      title: 'Delete session',
      message: 'This will permanently delete the session and all its data. This cannot be undone.',
      confirmLabel: 'Delete'
    })
    if (!ok) return
    await runNotifiedTask(() => removeSession(session.id, session.project_id), {
      loading: { title: 'Deleting session', description: session.id.slice(0, 8) },
      success: { title: 'Session deleted', description: session.id.slice(0, 8) },
      error: { title: 'Delete session failed' }
    })
  }
</script>

<SidebarPanel title="Projects">
  {#snippet headerActions()}
    <IconButton tooltip={sort === 'recent' ? 'Sort: recent activity' : 'Sort: name'} onclick={toggleSort}>
      <ArrowDownUpIcon size={12} />
    </IconButton>
  {/snippet}
  {#snippet headerExtra()}
    <InfoTooltip ariaLabel="Explain session status dots" contentClass="w-64">
      <div class="space-y-2">
        <p class="font-medium text-bright">Session status dots</p>
        <div class="grid grid-cols-[auto_1fr] items-center gap-x-2 gap-y-1.5">
          {#each SESSION_STATUS_LEGEND as item (item.label)}
            <span class="h-1.5 w-1.5 rounded-full {item.dot}"></span>
            <span>{item.label}</span>
          {/each}
        </div>
      </div>
    </InfoTooltip>
  {/snippet}

  <div class="flex h-full min-h-0 flex-col bg-ground">
    <SidebarRow
      variant="action"
      divider={false}
      class="h-7 shrink-0 border-b border-edge bg-ground"
      onclick={() => void openProjectDirectory()}
    >
      <FolderOpen size={12} class="shrink-0" />
      <span>New project…</span>
      <Plus size={12} class="ml-auto shrink-0" />
    </SidebarRow>

    <div class="min-h-0 flex-1 overflow-hidden">
      <ResizablePaneGroup direction="vertical">
        <!-- Live projects + their active sessions. -->
        <ResizablePane defaultSize={70} minSize={20}>
          <div class="h-full overflow-y-auto text-base">
            {#if sortedProjects.length === 0}
              <SidebarRow variant="info">No projects yet.</SidebarRow>
            {:else}
              {#each sortedProjects as project (project.id)}
                {@render projectSection(project, false)}
              {/each}
            {/if}
          </div>
        </ResizablePane>

        <ResizableHandle />

        <!-- Archived division: same rows, but only projects with archives
             and only their archived sessions. -->
        <ResizablePane defaultSize={30} minSize={10}>
          <div class="h-full overflow-y-auto text-base">
            <div class="px-2.5 py-1.5 text-2xs font-semibold tracking-wider text-subtle uppercase">
              Archived
              {#if archivedTotal > 0}<span class="font-normal">({archivedTotal})</span>{/if}
            </div>
            {#if archivedProjects.length === 0}
              <SidebarRow variant="info" divider={false}>No archived sessions.</SidebarRow>
            {:else}
              {#each archivedProjects as project (project.id)}
                {@render projectSection(project, true)}
              {/each}
            {/if}
          </div>
        </ResizablePane>
      </ResizablePaneGroup>
    </div>
  </div>
</SidebarPanel>

{#snippet projectSection(project: Project, archived: boolean)}
  {@const expanded = archived ? expandedArchived.has(project.id) : isProjectExpanded(project.id)}
  {@const sessions = archived ? getArchivedAgentSessions(project.id) : getAgentSessions(project.id)}
  {@const latest = sessions[0] ?? null}
  {@const active = project.id === activeProjectId}
  <ContextMenuRoot>
    <ContextMenuTrigger class="contents">
      <button
        type="button"
        class={cn(sidebarRowVariants({ variant: 'section' }), 'relative')}
        onclick={() => toggleProjectRow(project.id, archived)}
        ondblclick={() => openProject(project.id)}
        onauxclick={(e) => e.button === 1 && openProject(project.id)}
        aria-expanded={expanded}
      >
        {#if active}
          <TabBeam position="left" />
        {/if}
        <span class="min-w-0 flex-1 truncate text-fg">{project.name}</span>
        {#if latest}
          <span class="shrink-0 truncate text-2xs text-subtle">{timeAgo(latest.updated_at)}</span>
        {/if}
        <span class="h-1.5 w-1.5 shrink-0 rounded-full {latest ? sessionDotClass(latest) : 'bg-muted'}"></span>
      </button>
    </ContextMenuTrigger>
    <ContextMenuContent>
      <ContextMenuItem onclick={() => openProject(project.id)}>
        <FolderOpen size={14} class="shrink-0 text-muted" />
        <span>Open project</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={() => handleReveal(project)}>
        <FolderOpen size={14} class="shrink-0 text-muted" />
        <span>Reveal in file manager</span>
      </ContextMenuItem>
      <ContextMenuItem onclick={() => void handleProjectSettings(project)}>
        <SettingsIcon size={14} class="shrink-0 text-muted" />
        <span>Project settings</span>
      </ContextMenuItem>
      <ContextMenuSeparator />
      <ContextMenuItem onclick={() => void closeProject(project.id)}>
        <span>Close project</span>
      </ContextMenuItem>
      <ContextMenuItem destructive onclick={() => void handleRemoveProject(project)}>
        <Trash2 size={14} class="shrink-0" />
        <span>Remove from Sworm…</span>
      </ContextMenuItem>
    </ContextMenuContent>
  </ContextMenuRoot>

  {#if expanded}
    {@const key = (archived ? 'a:' : 'l:') + project.id}
    {@const showAll = showAllSessions.has(key)}
    {@const shown = showAll ? sessions : sessions.slice(0, SESSION_PREVIEW_LIMIT)}
    <!-- Darker grouped view for an expanded project's sessions, mirroring
         the issues sidebar; no file-tree indent guides. -->
    <div class="border-t border-edge/30 bg-surface">
      {#if sessions.length === 0}
        <SidebarRow variant="info" divider={false}>No sessions yet.</SidebarRow>
      {:else}
        {#each shown as session (session.id)}
          {@render sessionRow(session, archived)}
        {/each}
        {#if sessions.length > SESSION_PREVIEW_LIMIT}
          <SidebarRow variant="action" divider={false} onclick={() => toggleShowAll(key)}>
            <span>{showAll ? 'Show less' : `Show ${sessions.length - SESSION_PREVIEW_LIMIT} more`}</span>
          </SidebarRow>
        {/if}
      {/if}
      {#if !archived}
        <SidebarRow variant="action" divider={false} onclick={() => handleNewSession(project)}>
          <span>New session…</span>
          <Plus size={12} class="ml-auto shrink-0" />
        </SidebarRow>
      {/if}
    </div>
  {/if}
{/snippet}

{#snippet sessionRow(session: Session, archived: boolean)}
  {@const active = !archived && session.id === activeSessionId}
  {@const providerIcon = providerMap.get(session.provider_id)?.icon ?? null}
  <ContextMenuRoot>
    <ContextMenuTrigger class="contents">
      <button
        type="button"
        class={cn(
          sidebarRowVariants({ variant: 'leaf', pressed: active, divider: false }),
          'group/row',
          archived && 'opacity-60'
        )}
        onclick={() => {
          if (!archived) handleSessionClick(session)
        }}
      >
        {#if providerIcon}
          <img src={providerIcon} alt="" class="h-3 w-3 shrink-0 rounded-sm" />
        {/if}
        <span class="min-w-0 flex-1 truncate text-fg group-hover/row:text-bright">{session.title}</span>
        <span class="shrink-0 text-2xs text-subtle">{timeAgo(session.updated_at)}</span>
        <span class="h-1.5 w-1.5 shrink-0 rounded-full {sessionDotClass(session)}"></span>
      </button>
    </ContextMenuTrigger>
    <ContextMenuContent>
      {#if archived}
        <ContextMenuItem onclick={() => void handleUnarchive(session)}>
          <ArchiveRestoreIcon size={14} class="shrink-0 text-muted" />
          <span>Restore session</span>
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem destructive onclick={() => void handleDeleteSession(session)}>
          <Trash2 size={14} class="shrink-0" />
          <span>Delete session…</span>
        </ContextMenuItem>
      {:else}
        <ContextMenuItem onclick={() => handleSessionClick(session)}>
          <span>Open session</span>
        </ContextMenuItem>
        <ContextMenuItem onclick={() => void handleArchive(session)}>
          <ArchiveIcon size={14} class="shrink-0 text-muted" />
          <span>Archive session</span>
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem destructive onclick={() => void handleDeleteSession(session)}>
          <Trash2 size={14} class="shrink-0" />
          <span>Delete session…</span>
        </ContextMenuItem>
      {/if}
    </ContextMenuContent>
  </ContextMenuRoot>
{/snippet}
