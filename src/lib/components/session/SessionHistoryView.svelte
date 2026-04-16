<script lang="ts">
  import {
    getSessions,
    getArchivedSessions,
    loadArchivedSessions,
    archiveSession,
    unarchiveSession,
    removeSession
  } from '$lib/stores/sessions.svelte'
  import { addSessionTab, closeTabBySessionId, getAllTabs } from '$lib/stores/workspace.svelte'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import type { Session } from '$lib/types/backend'
  import { providerLabel } from '$lib/utils/session'
  import { getActivity } from '$lib/stores/activity.svelte'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import SidebarPanel from '$lib/components/SidebarPanel.svelte'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import { runNotifiedTask } from '$lib/utils/notifiedTask'

  let {
    projectId
  }: {
    projectId: string
  } = $props()

  let sessions = $derived(getSessions())
  let archivedSessions = $derived(getArchivedSessions())

  // Context menu state
  let ctxOpen = $state(false)
  let ctxSession = $state<Session | null>(null)
  let ctxIsArchived = $state(false)
  let ctxPos = $state({ x: 0, y: 0 })

  // Delete confirmation
  let deleteConfirmOpen = $state(false)
  let pendingDeleteId = $state<string | null>(null)

  // Load archived sessions on mount and when project changes
  $effect(() => {
    void loadArchivedSessions(projectId)
  })

  // Filter out terminal and fresh — only track agent CLIs
  let agentSessions = $derived(sessions.filter((s) => s.provider_id !== 'terminal' && s.provider_id !== 'fresh'))
  let agentArchived = $derived(
    archivedSessions.filter((s) => s.provider_id !== 'terminal' && s.provider_id !== 'fresh')
  )

  // Group sessions by provider
  function groupByProvider(list: Session[]): Map<string, Session[]> {
    const groups = new Map<string, Session[]>()
    for (const session of list) {
      const existing = groups.get(session.provider_id)
      if (existing) {
        existing.push(session)
      } else {
        groups.set(session.provider_id, [session])
      }
    }
    return groups
  }

  /**
   * Session dot class for session history.
   * Always visible (never hidden):
   *   working  → accent   (agent thinking/executing)
   *   waiting  → warning  (agent waiting for user)
   *   failed   → danger   (red)
   *   running/completed → success (green, "open" or "done")
   *   stopped/exited/idle → muted (grey, closed)
   */
  function historyDotClass(session: Session): string {
    if (session.status === 'failed') return 'bg-danger'

    if (session.status === 'running' || session.status === 'starting') {
      const activity = getActivity(session.id)
      if (activity === 'working') return 'bg-accent'
      if (activity === 'waiting') return 'bg-warning'
      return 'bg-success'
    }

    if (session.status === 'idle') return 'bg-success'

    return 'bg-muted'
  }

  let grouped = $derived(groupByProvider(agentSessions))
  let archivedGrouped = $derived(groupByProvider(agentArchived))

  function handleSessionClick(session: Session) {
    addSessionTab(projectId, session.id, session.title, session.provider_id)
  }

  function handleContextMenu(e: MouseEvent, session: Session, archived: boolean) {
    e.preventDefault()
    ctxSession = session
    ctxIsArchived = archived
    ctxPos = { x: e.clientX, y: e.clientY }
    ctxOpen = true
  }

  function closeContextMenu() {
    ctxOpen = false
    ctxSession = null
  }

  function isSessionTabLocked(sessionId: string): boolean {
    const tabs = getAllTabs(projectId)
    const tab = tabs.find((t) => t.kind === 'session' && t.sessionId === sessionId)
    return tab?.locked ?? false
  }

  async function handleArchive(session: Session) {
    if (isSessionTabLocked(session.id)) return
    closeContextMenu()
    const archived = await runNotifiedTask(() => archiveSession(session.id, projectId), {
      loading: { title: 'Archiving session', description: session.id.slice(0, 8) },
      success: { title: 'Session archived', description: session.id.slice(0, 8) },
      error: { title: 'Archive session failed' }
    })
    if (archived) {
      closeTabBySessionId(projectId, session.id)
    }
  }

  async function handleUnarchive(session: Session) {
    closeContextMenu()
    await runNotifiedTask(() => unarchiveSession(session.id, projectId), {
      loading: { title: 'Restoring session', description: session.id.slice(0, 8) },
      success: { title: 'Session restored', description: session.id.slice(0, 8) },
      error: { title: 'Restore session failed' }
    })
  }

  function handleDeleteRequest(session: Session) {
    closeContextMenu()
    pendingDeleteId = session.id
    deleteConfirmOpen = true
  }

  async function confirmDelete() {
    if (pendingDeleteId) {
      const sessionId = pendingDeleteId
      await runNotifiedTask(() => removeSession(sessionId, projectId), {
        loading: { title: 'Deleting session', description: sessionId.slice(0, 8) },
        success: { title: 'Session deleted', description: sessionId.slice(0, 8) },
        error: { title: 'Delete session failed' }
      })
    }
    deleteConfirmOpen = false
    pendingDeleteId = null
  }

  // Close context menu on outside events
  $effect(() => {
    if (!ctxOpen) return
    const close = () => closeContextMenu()
    const onKeydown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close()
    }
    window.addEventListener('blur', close)
    window.addEventListener('scroll', close, true)
    window.addEventListener('keydown', onKeydown)
    return () => {
      window.removeEventListener('blur', close)
      window.removeEventListener('scroll', close, true)
      window.removeEventListener('keydown', onKeydown)
    }
  })
</script>

{#snippet sessionList(list: Map<string, Session[]>, archived: boolean)}
  {#each [...list.entries()] as [providerId, providerSessions]}
    <div class="border-b border-edge last:border-b-0">
      <div class="px-2.5 py-1.5 text-[0.65rem] font-semibold tracking-wider text-muted uppercase">
        {providerLabel(providerId)}
      </div>
      {#each providerSessions as session}
        <button
          class="group flex w-full cursor-pointer items-center gap-2 px-2.5 py-1.5 text-left text-[0.75rem] text-subtle transition-colors hover:bg-surface hover:text-bright"
          onclick={archived ? undefined : () => handleSessionClick(session)}
          oncontextmenu={(e) => handleContextMenu(e, session, archived)}
        >
          {#if !archived}
            <span class="h-1.5 w-1.5 shrink-0 rounded-full {historyDotClass(session)}"></span>
          {/if}
          <span class="truncate font-mono text-[0.7rem]">{session.id.slice(0, 8)}</span>
        </button>
      {/each}
    </div>
  {/each}
{/snippet}

<SidebarPanel title="Sessions">
  {#snippet headerExtra()}
    <InfoTooltip ariaLabel="Explain session status dots" contentClass="w-64">
      <div class="space-y-2">
        <p class="font-medium text-bright">Session status dots</p>
        <div class="grid grid-cols-[auto_1fr] items-center gap-x-2 gap-y-1.5">
          <span class="h-1.5 w-1.5 rounded-full bg-success"></span>
          <span>Open / completed</span>
          <span class="h-1.5 w-1.5 rounded-full bg-accent"></span>
          <span>Agent working</span>
          <span class="h-1.5 w-1.5 rounded-full bg-warning"></span>
          <span>Waiting for input</span>
          <span class="h-1.5 w-1.5 rounded-full bg-danger"></span>
          <span>Failed</span>
          <span class="h-1.5 w-1.5 rounded-full bg-muted"></span>
          <span>Closed / exited</span>
        </div>
      </div>
    </InfoTooltip>
  {/snippet}

  <div class="min-h-0 flex-1">
    <ResizablePaneGroup direction="vertical">
      <ResizablePane defaultSize={65} minSize={20}>
        <div class="h-full overflow-y-auto">
          {#if agentSessions.length === 0}
            <div class="px-2.5 py-3 text-[0.75rem] text-subtle">No agent sessions yet.</div>
          {:else}
            {@render sessionList(grouped, false)}
          {/if}
        </div>
      </ResizablePane>
      <ResizableHandle />
      <ResizablePane defaultSize={35} minSize={10}>
        <div class="h-full overflow-y-auto">
          <div class="px-2.5 py-1.5 text-[0.65rem] font-semibold tracking-wider text-subtle uppercase">
            Archived
            {#if agentArchived.length > 0}
              <span class="text-[0.6rem] font-normal text-subtle">({agentArchived.length})</span>
            {/if}
          </div>
          {#if agentArchived.length === 0}
            <div class="px-2.5 py-1.5 text-[0.7rem] text-subtle">None</div>
          {:else}
            {@render sessionList(archivedGrouped, true)}
          {/if}
        </div>
      </ResizablePane>
    </ResizablePaneGroup>
  </div>
</SidebarPanel>

<!-- Context menu -->
{#if ctxOpen && ctxSession}
  <div class="fixed inset-0 z-40" onclick={closeContextMenu} role="none"></div>
  <div
    class="fixed z-50 min-w-[140px] rounded-lg border border-edge bg-raised py-1 text-[0.78rem] shadow-[0_8px_24px_rgba(0,0,0,0.4)]"
    style="left: {ctxPos.x}px; top: {ctxPos.y}px;"
  >
    {#if !ctxIsArchived}
      {@const locked = ctxSession ? isSessionTabLocked(ctxSession.id) : false}
      <button
        class="w-full rounded-sm border-none bg-transparent px-3 py-1.5 text-left transition-colors
          {locked ? 'cursor-default text-muted/50' : 'cursor-pointer text-fg hover:bg-surface'}"
        disabled={locked}
        onclick={() => ctxSession && handleArchive(ctxSession)}
      >
        Archive
      </button>
    {:else}
      <button
        class="w-full cursor-pointer rounded-sm border-none bg-transparent px-3 py-1.5 text-left text-fg transition-colors hover:bg-surface"
        onclick={() => ctxSession && handleUnarchive(ctxSession)}
      >
        Restore
      </button>
      <div class="mx-2 my-1 h-px bg-edge"></div>
      <button
        class="w-full cursor-pointer rounded-sm border-none bg-transparent px-3 py-1.5 text-left text-danger transition-colors hover:bg-danger-bg"
        onclick={() => ctxSession && handleDeleteRequest(ctxSession)}
      >
        Delete
      </button>
    {/if}
  </div>
{/if}

<ConfirmDialog
  open={deleteConfirmOpen}
  title="Delete Session"
  message="This will permanently delete the session and all its data. This cannot be undone."
  confirmLabel="Delete"
  onCancel={() => {
    deleteConfirmOpen = false
    pendingDeleteId = null
  }}
  onConfirm={confirmDelete}
/>
