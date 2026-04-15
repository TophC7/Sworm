<script lang="ts">
  import { backend } from '$lib/api/backend'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import { TabButton, TabStrip } from '$lib/components/ui/chrome-tabs'
  import { allProviders, directOptions } from '$lib/data/providers'
  import { getSessions, updateSessionInList } from '$lib/stores/sessions.svelte'
  import type { PaneSlot, Tab, TabId } from '$lib/stores/workspace.svelte'
  import {
    closeTab,
    collapsePaneIfEmpty,
    endTabDrag,
    promoteTemporaryTab,
    setActiveTab,
    startTabDrag,
    toggleTabLocked
  } from '$lib/stores/workspace.svelte'
  import * as sessionRegistry from '$lib/terminal/sessionRegistry'
  import { IconButton } from '$lib/components/ui/button'
  import { FileDiff, GitCommitIcon, PackageIcon, Lock, Plus } from '$lib/icons/lucideExports'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import { tick } from 'svelte'

  let {
    tabs,
    activeTabId,
    paneSlot,
    projectId,
    isFocused = false,
    onNewSession,
    onTabSelected
  }: {
    tabs: Tab[]
    activeTabId: TabId | null
    paneSlot: PaneSlot
    projectId: string
    isFocused?: boolean
    onNewSession?: () => void
    onTabSelected?: () => void
  } = $props()

  let contextMenuOpen = $state(false)
  let contextMenuTabId = $state<TabId | null>(null)
  let contextMenuPos = $state({ x: 0, y: 0 })
  let sessions = $derived(getSessions())
  let warningOpen = $state(false)
  let pendingSessionAction: (() => Promise<void>) | null = null

  function handleTabClick(tabId: TabId) {
    setActiveTab(projectId, paneSlot, tabId)
    onTabSelected?.()
  }

  async function handleTabClose(e: Event, tabId: TabId) {
    e.stopPropagation()

    const tab = tabs.find((candidate) => candidate.id === tabId)
    if (!tab) return

    if (tab.kind === 'session') {
      // Stop the PTY if actively running, but keep the session record in DB
      // so it appears in the session history view
      const session = sessions.find((s) => s.id === tab.sessionId)
      if (session && (session.status === 'running' || session.status === 'starting')) {
        try {
          await backend.sessions.stop(tab.sessionId)
          updateSessionInList(tab.sessionId, { status: 'stopped' })
        } catch (err) {
          console.error('Failed to stop session on tab close:', err)
        }
      }
    }

    closeTab(projectId, tabId)
    collapsePaneIfEmpty(projectId)
  }

  function handleContextMenu(e: MouseEvent, tabId: TabId) {
    e.preventDefault()
    contextMenuTabId = tabId
    contextMenuPos = { x: e.clientX, y: e.clientY }
    contextMenuOpen = true
  }

  function closeContextMenu() {
    contextMenuOpen = false
    contextMenuTabId = null
  }

  function tabLabel(tab: Tab): string {
    switch (tab.kind) {
      case 'session':
        return tab.title
      case 'commit':
        return tab.shortHash
      case 'changes':
        return tab.label
      case 'stash':
        return `stash@{${tab.stashIndex}}`
      case 'markdown':
        return tab.fileName
    }
  }

  function providerIcon(tab: Tab): string | null {
    if (tab.kind !== 'session') return null
    const provider =
      allProviders.find((p) => p.id === tab.providerId) ?? directOptions.find((p) => p.id === tab.providerId)
    return provider?.icon ?? null
  }

  function handleDragStart(e: DragEvent, tabId: TabId) {
    const tab = tabs.find((candidate) => candidate.id === tabId)
    if (!tab || tab.locked) {
      e.preventDefault()
      return
    }

    startTabDrag(projectId, tabId)
    e.dataTransfer?.setData('text/plain', tabId)
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move'
    }
  }

  function handleDragEnd() {
    endTabDrag()
  }

  function handleAuxClick(e: MouseEvent, tabId: TabId) {
    if (e.button !== 1) return
    void handleTabClose(e, tabId)
  }

  function getTabById(tabId: TabId | null): Tab | null {
    if (!tabId) return null
    return tabs.find((candidate) => candidate.id === tabId) ?? null
  }

  function contextSession(tabId: TabId | null) {
    const tab = getTabById(tabId)
    if (!tab || tab.kind !== 'session') return null
    return sessions.find((session) => session.id === tab.sessionId) ?? null
  }

  async function stopSessionFromMenu(tabId: TabId) {
    const tab = getTabById(tabId)
    if (!tab || tab.kind !== 'session') return

    try {
      const manager = sessionRegistry.get(tab.sessionId)
      if (manager) {
        await manager.stopPty()
      } else {
        await backend.sessions.stop(tab.sessionId)
      }
      updateSessionInList(tab.sessionId, { status: 'stopped' })
    } catch (error) {
      console.error('Failed to stop session:', error)
    } finally {
      closeContextMenu()
    }
  }

  async function restartSessionNow(tabId: TabId) {
    const tab = getTabById(tabId)
    const session = contextSession(tabId)
    if (!tab || tab.kind !== 'session' || !session) return

    setActiveTab(projectId, paneSlot, tab.id)
    onTabSelected?.()
    await tick()

    try {
      const manager = sessionRegistry.getOrCreate(session.id)
      if (manager.isPtyActive()) {
        await manager.stopPty()
        updateSessionInList(session.id, { status: 'stopped' })
      }
      await manager.startPty(session)
      updateSessionInList(session.id, { status: 'running' })
    } catch (error) {
      console.error('Failed to restart session:', error)
      updateSessionInList(session.id, { status: 'failed' })
    } finally {
      closeContextMenu()
    }
  }

  function handleRestartFromMenu(tabId: TabId) {
    const session = contextSession(tabId)
    if (!session) return

    const restart = () => restartSessionNow(tabId)
    if (sessions.some((candidate) => candidate.id !== session.id && candidate.status === 'running')) {
      pendingSessionAction = restart
      warningOpen = true
      closeContextMenu()
      return
    }

    void restart()
  }

  function handleToggleLock(tabId: TabId) {
    toggleTabLocked(projectId, tabId)
    closeContextMenu()
  }

  $effect(() => {
    if (!contextMenuOpen) return
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

<TabStrip variant="pane" ariaLabel="Pane tabs" class={isFocused ? 'border-edge' : 'border-edge/50'}>
  {#each tabs as tab (tab.id)}
    <TabButton
      variant="pane"
      active={activeTabId === tab.id}
      draggable={!tab.locked}
      onclick={() => handleTabClick(tab.id)}
      ondblclick={() => {
        if (tab.kind !== 'session' && tab.temporary) promoteTemporaryTab(tab.id)
      }}
      oncontextmenu={(e) => handleContextMenu(e, tab.id)}
      onauxclick={(e) => handleAuxClick(e, tab.id)}
      ondragstart={(e) => handleDragStart(e, tab.id)}
      ondragend={handleDragEnd}
      onClose={tab.locked ? undefined : (e) => handleTabClose(e, tab.id)}
    >
      {#snippet leading()}
        {#if tab.kind === 'changes'}
          <FileDiff size={14} class="shrink-0 text-accent" />
        {:else if tab.kind === 'commit'}
          <GitCommitIcon size={14} class="shrink-0 text-accent" />
        {:else if tab.kind === 'stash'}
          <PackageIcon size={14} class="shrink-0 text-accent" />
        {:else if tab.kind === 'markdown'}
          <FileIcon filename={tab.fileName} size={14} />
        {:else}
          {@const icon = providerIcon(tab)}
          {#if icon}
            <img src={icon} alt="" width={14} height={14} class="shrink-0" />
          {/if}
        {/if}
        {#if tab.locked}
          <Lock size={11} class="shrink-0 text-muted" />
        {/if}
      {/snippet}
      <span class="max-w-[120px] truncate {tab.kind !== 'session' && tab.temporary ? 'italic' : ''}">
        {tabLabel(tab)}
      </span>
    </TabButton>
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

{#if contextMenuOpen}
  {@const contextTab = getTabById(contextMenuTabId)}
  {@const contextSessionValue = contextSession(contextMenuTabId)}
  <div class="fixed inset-0 z-40" onclick={closeContextMenu} role="none"></div>
  <div
    class="fixed z-50 min-w-[140px] rounded-lg border border-edge bg-raised py-1 text-[0.78rem] shadow-[0_8px_24px_rgba(0,0,0,0.4)]"
    style="left: {contextMenuPos.x}px; top: {contextMenuPos.y}px;"
  >
    {#if contextTab?.kind === 'session' && contextSessionValue}
      <button
        class="w-full cursor-pointer rounded-sm border-none bg-transparent px-3 py-1.5 text-left text-fg transition-colors hover:bg-surface"
        onclick={() =>
          contextSessionValue.status === 'running' || contextSessionValue.status === 'starting'
            ? void stopSessionFromMenu(contextTab.id)
            : handleRestartFromMenu(contextTab.id)}
      >
        {contextSessionValue.status === 'running' || contextSessionValue.status === 'starting' ? 'Stop' : 'Restart'}
      </button>
    {/if}
    {#if contextTab}
      <button
        class="w-full cursor-pointer rounded-sm border-none bg-transparent px-3 py-1.5 text-left text-fg transition-colors hover:bg-surface"
        onclick={() => handleToggleLock(contextTab.id)}
      >
        {contextTab.locked ? 'Unlock Tab' : 'Lock Tab'}
      </button>
      <div class="mx-2 my-1 h-px bg-edge"></div>
    {/if}
    <button
      class="w-full rounded-sm border-none bg-transparent px-3 py-1.5 text-left transition-colors
				{contextTab?.locked ? 'cursor-default text-muted/50' : 'cursor-pointer text-danger hover:bg-danger-bg'}"
      disabled={contextTab?.locked}
      onclick={(e) => {
        if (contextMenuTabId) {
          void handleTabClose(e, contextMenuTabId)
        }
        closeContextMenu()
      }}>Close</button
    >
  </div>
{/if}

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
