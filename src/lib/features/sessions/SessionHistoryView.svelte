<script lang="ts">
  import {
    getSessions,
    getArchivedSessions,
    loadArchivedSessions,
    archiveSession,
    unarchiveSession,
    removeSession
  } from '$lib/features/sessions/state/sessions.svelte'
  import { closeTabBySessionId, getAllTabs } from '$lib/features/workbench/state.svelte'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { IconButton } from '$lib/components/ui/button'
  import { ArchiveIcon, ArchiveRestoreIcon, Trash2 } from '$lib/icons/lucideExports'
  import type { Session } from '$lib/types/backend'
  import { providerLabel } from '$lib/features/sessions/providers/labels'
  import { getActivity } from '$lib/features/sessions/state/sessionActivity.svelte'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import SidebarPanel from '$lib/features/app-shell/sidebar/SidebarPanel.svelte'
  import ConfirmDialog from '$lib/components/dialogs/ConfirmDialog.svelte'
  import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
  import { ensureSessionSurface } from '$lib/features/workbench/surfaces/session/service.svelte'

  let {
    projectId
  }: {
    projectId: string
  } = $props()

  let sessions = $derived(getSessions())
  let archivedSessions = $derived(getArchivedSessions())

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

  // Set of session ids whose tabs are pinned/locked. Derived once per
  // tab change so the per-row archive button can read O(1) instead of
  // scanning all tabs N times during render.
  let lockedSessionIds = $derived.by(() => {
    const ids = new Set<string>()
    for (const t of getAllTabs(projectId)) {
      if (t.kind === 'session' && t.locked) ids.add(t.sessionId)
    }
    return ids
  })

  function handleSessionClick(session: Session) {
    void ensureSessionSurface(projectId, session.id, session.title, session.provider_id)
  }

  async function handleArchive(session: Session) {
    if (lockedSessionIds.has(session.id)) return
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
    await runNotifiedTask(() => unarchiveSession(session.id, projectId), {
      loading: { title: 'Restoring session', description: session.id.slice(0, 8) },
      success: { title: 'Session restored', description: session.id.slice(0, 8) },
      error: { title: 'Restore session failed' }
    })
  }

  function handleDeleteRequest(session: Session) {
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
</script>

{#snippet sessionList(list: Map<string, Session[]>, archived: boolean)}
  {#each [...list.entries()] as [providerId, providerSessions]}
    <div class="border-b border-edge last:border-b-0">
      <div class="px-2.5 py-1.5 text-2xs font-semibold tracking-wider text-muted uppercase">
        {providerLabel(providerId)}
      </div>
      {#each providerSessions as session}
        {@const locked = !archived && lockedSessionIds.has(session.id)}
        <div
          class="group/row relative flex items-center text-sm text-subtle transition-colors hover:bg-surface hover:text-bright"
        >
          {#if archived}
            <!--
              Archived rows are not interactive on click — the only
              affordances are the hover restore + delete buttons. Render
              as a div so it doesn't appear focusable/clickable when it
              isn't.
            -->
            <div class="flex flex-1 items-center gap-2 px-2.5 py-1.5 text-left">
              <span class="truncate font-mono text-xs">{session.id.slice(0, 8)}</span>
            </div>
          {:else}
            <button
              type="button"
              class="flex flex-1 cursor-pointer items-center gap-2 px-2.5 py-1.5 text-left"
              onclick={() => handleSessionClick(session)}
            >
              <span class="h-1.5 w-1.5 shrink-0 rounded-full {historyDotClass(session)}"></span>
              <span class="truncate font-mono text-xs">{session.id.slice(0, 8)}</span>
            </button>
          {/if}
          <!--
            Hover-revealed actions mirror the GitFileTree header pattern
            (group/hdr + opacity transition). Live rows expose archive;
            archived rows expose restore + delete so destructive removal
            is gated behind a deliberate archive-first workflow.
          -->
          <div
            class="absolute right-1 flex items-center gap-0.5 pr-1 opacity-0 transition-opacity group-hover/row:opacity-100"
          >
            {#if archived}
              <IconButton
                tooltip="Restore session"
                tooltipSide="left"
                class="rounded p-0.5 text-muted hover:text-fg"
                onclick={() => handleUnarchive(session)}
              >
                <ArchiveRestoreIcon size={12} />
              </IconButton>
              <IconButton
                tooltip="Delete session"
                tooltipSide="left"
                class="rounded p-0.5 text-muted hover:text-danger"
                onclick={() => handleDeleteRequest(session)}
              >
                <Trash2 size={12} />
              </IconButton>
            {:else}
              <IconButton
                tooltip={locked ? 'Unlock session tab to archive' : 'Archive session'}
                tooltipSide="left"
                class="rounded p-0.5 text-muted hover:text-fg"
                disabled={locked}
                onclick={() => handleArchive(session)}
              >
                <ArchiveIcon size={12} />
              </IconButton>
            {/if}
          </div>
        </div>
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

  <ResizablePaneGroup direction="vertical">
    <ResizablePane defaultSize={65} minSize={20}>
      <div class="h-full overflow-y-auto">
        {#if agentSessions.length === 0}
          <div class="px-2.5 py-3 text-sm text-subtle">No agent sessions yet.</div>
        {:else}
          {@render sessionList(grouped, false)}
        {/if}
      </div>
    </ResizablePane>
    <ResizableHandle />
    <ResizablePane defaultSize={35} minSize={10}>
      <div class="h-full overflow-y-auto">
        <div class="px-2.5 py-1.5 text-2xs font-semibold tracking-wider text-subtle uppercase">
          Archived
          {#if agentArchived.length > 0}
            <span class="text-2xs font-normal text-subtle">({agentArchived.length})</span>
          {/if}
        </div>
        {#if agentArchived.length === 0}
          <div class="px-2.5 py-1.5 text-xs text-subtle">None</div>
        {:else}
          {@render sessionList(archivedGrouped, true)}
        {/if}
      </div>
    </ResizablePane>
  </ResizablePaneGroup>
</SidebarPanel>

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
