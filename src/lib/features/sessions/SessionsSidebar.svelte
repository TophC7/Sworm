<!--
  @component
  SessionsSidebar. Agent sessions for the current project: one flat
  recency-ordered list, no provider grouping, with the project's
  archived sessions in a resizable division below. Project switching
  lives in the ProjectStrip, so this view is strictly project-scoped.

  Click rules:
    - Click a session row -> open it as a tab.
    - Right-click a row   -> context-menu actions.
-->

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import SidebarPanel from '$lib/features/app-shell/sidebar/SidebarPanel.svelte'
  import { SidebarRow, sidebarRowVariants } from '$lib/components/ui/sidebar-row'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import {
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuRoot,
    ContextMenuSeparator,
    ContextMenuTrigger
  } from '$lib/components/ui/context-menu'
  import { ArchiveIcon, ArchiveRestoreIcon, Plus, Trash2 } from '$lib/icons/lucideExports'
  import { getActiveSessionId, openLauncherTab, closeTabBySessionId } from '$lib/features/workbench/state.svelte'
  import {
    getAgentSessions,
    getArchivedAgentSessions,
    loadArchivedSessions,
    archiveSession,
    unarchiveSession,
    removeSession
  } from '$lib/features/sessions/state/sessions.svelte'
  import { ensureSessionSurface } from '$lib/features/workbench/surfaces/session/service.svelte'
  import { sessionDotClass, SESSION_STATUS_LEGEND } from '$lib/features/sessions/visual'
  import { allProviders, directOptions } from '$lib/features/sessions/providers/catalog'
  import { confirmAsync } from '$lib/features/confirm/service.svelte'
  import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
  import { timeAgo } from '$lib/utils/date'
  import type { Session } from '$lib/types/backend'

  let { projectId }: { projectId: string } = $props()

  // Provider glyph lookup; same source as DiscoveredProjectsPanel.
  const providerMap = new Map([...allProviders, ...directOptions].map((p) => [p.id, p]))

  let sessions = $derived(getAgentSessions(projectId))
  let archived = $derived(getArchivedAgentSessions(projectId))
  let activeSessionId = $derived(getActiveSessionId())

  // Live sessions load with the project (ProjectView); archived ones
  // are only needed here, so fetch them when the project changes.
  $effect(() => {
    void loadArchivedSessions(projectId)
  })

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

<SidebarPanel title="Sessions">
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
      onclick={() => openLauncherTab(projectId)}
    >
      <span>New session…</span>
      <Plus size={12} class="ml-auto shrink-0" />
    </SidebarRow>

    <div class="min-h-0 flex-1 overflow-hidden">
      <ResizablePaneGroup direction="vertical">
        <!-- Live sessions, latest first. -->
        <ResizablePane defaultSize={70} minSize={20}>
          <div class="h-full overflow-y-auto text-base">
            {#if sessions.length === 0}
              <SidebarRow variant="info" divider={false}>No sessions yet.</SidebarRow>
            {:else}
              {#each sessions as session (session.id)}
                {@render sessionRow(session, false)}
              {/each}
            {/if}
          </div>
        </ResizablePane>

        <ResizableHandle />

        <!-- Archived division. -->
        <ResizablePane defaultSize={30} minSize={10}>
          <div class="h-full overflow-y-auto text-base">
            <div class="px-2.5 py-1.5 text-2xs font-semibold tracking-wider text-subtle uppercase">
              Archived
              {#if archived.length > 0}<span class="font-normal">({archived.length})</span>{/if}
            </div>
            {#if archived.length === 0}
              <SidebarRow variant="info" divider={false}>No archived sessions.</SidebarRow>
            {:else}
              {#each archived as session (session.id)}
                {@render sessionRow(session, true)}
              {/each}
            {/if}
          </div>
        </ResizablePane>
      </ResizablePaneGroup>
    </div>
  </div>
</SidebarPanel>

{#snippet sessionRow(session: Session, isArchived: boolean)}
  {@const active = !isArchived && session.id === activeSessionId}
  {@const providerIcon = providerMap.get(session.provider_id)?.icon ?? null}
  <ContextMenuRoot>
    <ContextMenuTrigger class="contents">
      <button
        type="button"
        class={cn(
          sidebarRowVariants({ variant: 'leaf', pressed: active, divider: false }),
          'group/row',
          isArchived && 'opacity-60'
        )}
        onclick={() => {
          if (!isArchived) handleSessionClick(session)
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
      {#if isArchived}
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
