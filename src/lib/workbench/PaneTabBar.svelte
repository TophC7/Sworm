<script lang="ts">
  import { backend } from '$lib/api/backend'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import { TabButton, TabStrip } from '$lib/components/ui/chrome-tabs'
  import {
    ContextMenuRoot,
    ContextMenuTrigger,
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuSeparator
  } from '$lib/components/ui/context-menu'
  import { getSessions, updateSessionInList } from '$lib/stores/sessions.svelte'
  import type { PaneSlot, Tab, TabId } from '$lib/workbench/state.svelte'
  import { canLockTab, promoteTemporaryTab, setActiveTab, toggleTabLocked } from '$lib/workbench/state.svelte'
  import { tabDragSource } from '$lib/dnd/adapters/tab-strip'
  import * as sessionRegistry from '$lib/terminal/sessionRegistry'
  import { closeTabWithChecks } from '$lib/utils/tabActions.svelte'
  import { IconButton } from '$lib/components/ui/button'
  import { FileDiff, BellIcon, Lock, Plus } from '$lib/icons/lucideExports'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import { tick } from 'svelte'
  import { getErrorMessage, runNotifiedTask } from '$lib/utils/notifiedTask'
  import { getTabPresentation } from '$lib/workbench/presentation.svelte'
  import { getSurfaceKind } from '$lib/workbench/surfaces'

  let {
    tabs,
    activeTabId,
    paneSlot,
    projectId,
    isFocused = false,
    onNewSession
  }: {
    tabs: Tab[]
    activeTabId: TabId | null
    paneSlot: PaneSlot
    projectId: string
    isFocused?: boolean
    onNewSession?: () => void
  } = $props()

  let sessions = $derived(getSessions())
  let warningOpen = $state(false)
  let pendingSessionAction: (() => Promise<void>) | null = null

  function handleTabClick(tabId: TabId) {
    setActiveTab(projectId, paneSlot, tabId)
  }

  async function handleTabClose(e: Event, tabId: TabId) {
    e.stopPropagation()
    await closeTabWithChecks(projectId, tabId)
  }

  function handleAuxClick(e: MouseEvent, tabId: TabId) {
    if (e.button !== 1) return
    void handleTabClose(e, tabId)
  }

  function sessionForTab(tab: Tab) {
    if (tab.kind !== 'session') return null
    return sessions.find((session) => session.id === tab.sessionId) ?? null
  }

  async function stopSessionFromMenu(tab: Tab) {
    if (tab.kind !== 'session') return

    await runNotifiedTask(
      async () => {
        const manager = sessionRegistry.get(tab.sessionId)
        if (manager) {
          await manager.stopPty()
        } else {
          await backend.sessions.stop(tab.sessionId)
        }
        updateSessionInList(tab.sessionId, { status: 'stopped' })
      },
      {
        loading: { title: 'Stopping session', description: getTabPresentation(tab).title },
        success: { title: 'Session stopped', description: getTabPresentation(tab).title },
        error: { title: 'Stop session failed' }
      }
    )
  }

  async function restartSessionNow(tab: Tab) {
    const session = sessionForTab(tab)
    if (tab.kind !== 'session' || !session) return

    setActiveTab(projectId, paneSlot, tab.id)
    await tick()

    await runNotifiedTask(
      async () => {
        const manager = sessionRegistry.getOrCreate(session.id)
        if (manager.isPtyActive()) {
          await manager.stopPty()
          updateSessionInList(session.id, { status: 'stopped' })
        }
        await manager.startPty(session)
        updateSessionInList(session.id, { status: 'running' })
      },
      {
        loading: { title: 'Restarting session', description: getTabPresentation(tab).title },
        success: { title: 'Session restarted', description: getTabPresentation(tab).title },
        error: {
          title: 'Restart session failed',
          description: (error) => {
            updateSessionInList(session.id, { status: 'failed' })
            return getErrorMessage(error)
          }
        }
      }
    )
  }

  function handleRestartFromMenu(tab: Tab) {
    const session = sessionForTab(tab)
    if (!session) return

    const restart = () => restartSessionNow(tab)
    // Restarts share the same shared-workspace footgun as starts — prompt
    // the user if another session is live before kicking it.
    if (sessions.some((candidate) => candidate.id !== session.id && candidate.status === 'running')) {
      pendingSessionAction = restart
      warningOpen = true
      return
    }

    void restart()
  }

  function handleToggleLock(tabId: TabId) {
    toggleTabLocked(projectId, tabId)
  }
</script>

<TabStrip variant="pane" ariaLabel="Pane tabs" class={isFocused ? 'border-edge' : 'border-edge/50'}>
  {#each tabs as tab (tab.id)}
    {@const session = sessionForTab(tab)}
    {@const isRunning = session?.status === 'running' || session?.status === 'starting'}
    {@const presentation = getTabPresentation(tab)}
    {@const surfaceKind = getSurfaceKind(tab)}
    <ContextMenuRoot>
      <ContextMenuTrigger
        class="contents"
        draggable={!tab.locked}
        {@attach tabDragSource({ tab, paneSlot, projectId })}
      >
        <TabButton
          variant="pane"
          active={activeTabId === tab.id}
          draggable={!tab.locked}
          onclick={() => handleTabClick(tab.id)}
          ondblclick={() => {
            if (surfaceKind !== 'session' && surfaceKind !== 'launcher' && presentation.preview) {
              promoteTemporaryTab(tab.id)
            }
          }}
          onauxclick={(e) => handleAuxClick(e, tab.id)}
          onClose={tab.locked ? undefined : (e) => handleTabClose(e, tab.id)}
        >
          {#snippet leading()}
            {#if surfaceKind === 'diff'}
              <FileDiff size={14} class="shrink-0 text-accent" />
            {:else if surfaceKind === 'tool'}
              <BellIcon size={14} class="shrink-0 text-accent" />
            {:else if surfaceKind === 'text' && presentation.fileName}
              <FileIcon filename={presentation.fileName} size={14} />
            {:else if surfaceKind === 'launcher'}
              <Plus size={14} class="shrink-0 text-accent" />
            {:else if presentation.providerIcon}
              <img src={presentation.providerIcon} alt="" width={14} height={14} class="shrink-0" />
            {/if}
            {#if tab.locked}
              <Lock size={11} class="shrink-0 text-muted" />
            {/if}
          {/snippet}
          <span class="max-w-[120px] truncate {presentation.preview ? 'italic' : ''}">
            {presentation.title}
          </span>
        </TabButton>
      </ContextMenuTrigger>

      <ContextMenuContent>
        {#if tab.kind === 'session' && session}
          <ContextMenuItem onclick={() => (isRunning ? void stopSessionFromMenu(tab) : handleRestartFromMenu(tab))}>
            {isRunning ? 'Stop' : 'Restart'}
          </ContextMenuItem>
        {/if}
        {#if canLockTab(tab)}
          <!-- Lock only makes sense on content tabs where accidental input
               can cause damage (session terminals, Monaco text tabs). Launcher
               and diff tabs skip this affordance entirely. -->
          <ContextMenuItem onclick={() => handleToggleLock(tab.id)}>
            {tab.locked ? 'Unlock Tab' : 'Lock Tab'}
          </ContextMenuItem>
          <ContextMenuSeparator />
        {/if}
        <ContextMenuItem destructive disabled={tab.locked} onclick={() => void closeTabWithChecks(projectId, tab.id)}>
          Close
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenuRoot>
  {/each}

  {#snippet trailing()}
    <IconButton
      tooltip="New session"
      tooltipSide="bottom"
      class="sticky right-0 ml-0.5 flex h-7 w-7 shrink-0 cursor-pointer items-center justify-center border-none bg-ground text-sm text-muted transition-colors hover:text-bright"
      onclick={onNewSession}
    >
      <Plus size={14} />
    </IconButton>
  {/snippet}
</TabStrip>

<ConfirmDialog
  open={warningOpen}
  title="Shared Workspace Warning"
  message="Another session is already running in this project.\n\nSessions in the same project share the same working tree and branch.\nChanges made by one session may conflict with another."
  confirmLabel="Start Anyway"
  onCancel={() => {
    warningOpen = false
    pendingSessionAction = null
  }}
  onConfirm={() => {
    const action = pendingSessionAction
    warningOpen = false
    pendingSessionAction = null
    if (action) {
      void action()
    }
  }}
/>
