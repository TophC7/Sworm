<script lang="ts">
  import type { Tab, PaneState } from '$lib/stores/workspace.svelte'
  import {
    setFocusedPane,
    getFocusedPaneSlot,
    getDraggedTab,
    canSplitPane,
    moveTabToPane,
    splitPaneAt,
    setActiveTab,
    endTabDrag
  } from '$lib/stores/workspace.svelte'
  import PaneTabBar from '$lib/components/PaneTabBar.svelte'
  import SessionTerminal from '$lib/components/session/SessionTerminal.svelte'
  import ChangesView from '$lib/components/ChangesView.svelte'
  import CommitView from '$lib/components/CommitView.svelte'
  import StashView from '$lib/components/StashView.svelte'
  import NewSessionView from '$lib/components/session/NewSessionView.svelte'
  import { getSessions, updateSessionInList } from '$lib/stores/sessions.svelte'
  import { refreshGit } from '$lib/stores/git.svelte'

  type DropIntent = 'move' | 'right' | 'down' | null

  let {
    pane,
    tabs,
    projectId,
    projectPath
  }: {
    pane: PaneState
    tabs: Tab[]
    projectId: string
    projectPath: string
  } = $props()

  let isFocused = $derived(getFocusedPaneSlot() === pane.slot)
  let draggedTab = $derived(getDraggedTab())
  let canSplitRight = $derived(canSplitPane(projectId, pane.slot, 'right'))
  let canSplitDown = $derived(canSplitPane(projectId, pane.slot, 'down'))

  let sessions = $derived(getSessions())
  let activeTab = $derived(pane.activeTabId ? (tabs.find((tab) => tab.id === pane.activeTabId) ?? null) : null)
  let paneSession = $derived(
    activeTab?.kind === 'session' ? (sessions.find((session) => session.id === activeTab.sessionId) ?? null) : null
  )

  let showNewSession = $state(false)
  let showLockedOverlay = $derived(!showNewSession && activeTab !== null && activeTab.locked)
  let dropIntent = $state<DropIntent>(null)
  let dropActive = $state(false)

  function handleFocus() {
    setFocusedPane(pane.slot)
  }

  function handleNewSession() {
    handleFocus()
    showNewSession = true
  }

  function dragTabId(): string | null {
    if (!draggedTab || draggedTab.projectId !== projectId) return null
    return draggedTab.tabId
  }

  function computeDropIntent(event: DragEvent): DropIntent {
    if (!dragTabId()) return null

    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect()
    const x = rect.width > 0 ? (event.clientX - rect.left) / rect.width : 0
    const y = rect.height > 0 ? (event.clientY - rect.top) / rect.height : 0

    if (canSplitRight && x >= 0.72) return 'right'
    if (canSplitDown && y >= 0.72) return 'down'
    return 'move'
  }

  function handleDragOver(event: DragEvent) {
    if (!dragTabId()) return

    event.preventDefault()
    handleFocus()
    dropActive = true
    dropIntent = computeDropIntent(event)

    if (event.dataTransfer) {
      event.dataTransfer.dropEffect = 'move'
    }
  }

  function handleDragLeave(event: DragEvent) {
    const next = event.relatedTarget as Node | null
    if (next && (event.currentTarget as HTMLElement).contains(next)) return
    dropActive = false
    dropIntent = null
  }

  function handleDrop(event: DragEvent) {
    const tabId = dragTabId()
    if (!tabId) return

    event.preventDefault()
    handleFocus()
    showNewSession = false

    const intent = computeDropIntent(event)
    if (intent === 'move') {
      moveTabToPane(projectId, tabId, pane.slot)
      setActiveTab(projectId, pane.slot, tabId)
    } else if (intent) {
      const newSlot = splitPaneAt(projectId, pane.slot, intent)
      if (newSlot) {
        moveTabToPane(projectId, tabId, newSlot)
        setActiveTab(projectId, newSlot, tabId)
      }
    }

    dropActive = false
    dropIntent = null
    endTabDrag()
  }

  $effect(() => {
    if (!draggedTab) {
      dropActive = false
      dropIntent = null
    }
  })
</script>

<div
  class="relative flex h-full min-h-0 w-full min-w-0 flex-col overflow-hidden {isFocused
    ? 'ring-1 ring-accent/30'
    : ''}"
  onfocusin={handleFocus}
  onpointerdown={handleFocus}
  ondragover={handleDragOver}
  ondragleave={handleDragLeave}
  ondrop={handleDrop}
  role="region"
>
  <PaneTabBar
    {tabs}
    activeTabId={pane.activeTabId}
    paneSlot={pane.slot}
    {projectId}
    {isFocused}
    onNewSession={handleNewSession}
    onTabSelected={() => (showNewSession = false)}
  />

  <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden">
    {#if showNewSession || !activeTab}
      <NewSessionView onCreated={() => (showNewSession = false)} />
    {:else if activeTab.kind === 'session' && paneSession}
      <SessionTerminal
        session={paneSession}
        locked={activeTab.locked}
        onStatusChange={(status) => {
          updateSessionInList(paneSession.id, { status })
          if (status === 'exited' || status === 'stopped') {
            void refreshGit(projectId, projectPath)
          }
        }}
      />
    {:else if activeTab.kind === 'commit'}
      <CommitView commitHash={activeTab.commitHash} {projectId} {projectPath} initialFile={activeTab.initialFile} />
    {:else if activeTab.kind === 'changes'}
      <ChangesView {projectId} {projectPath} staged={activeTab.staged} initialFile={activeTab.initialFile} />
    {:else if activeTab.kind === 'stash'}
      <StashView stashIndex={activeTab.stashIndex} {projectId} {projectPath} initialFile={activeTab.initialFile} />
    {/if}

    {#if showLockedOverlay}
      <div class="absolute inset-0 z-10 flex items-center justify-center bg-ground/72 backdrop-blur-[1px]">
        <div
          class="rounded-lg border border-edge bg-surface/95 px-3 py-2 text-center text-[0.78rem] text-muted shadow-[0_8px_24px_rgba(0,0,0,0.35)]"
        >
          <div class="font-medium text-fg">Tab locked</div>
          <div>Unlock it from the tab menu to interact.</div>
        </div>
      </div>
    {/if}
  </div>

  {#if draggedTab?.projectId === projectId && dropActive}
    <div class="pointer-events-none absolute inset-0 z-20 p-2">
      <div class="absolute inset-2 rounded-xl border border-edge/70 bg-ground/55"></div>
      <div
        class="absolute inset-4 flex items-center justify-center rounded-lg border text-[0.72rem] tracking-wide uppercase
					{dropIntent === 'move' ? 'border-accent bg-accent/12 text-accent' : 'border-edge/70 bg-surface/45 text-muted'}"
      >
        Move Here
      </div>

      {#if canSplitRight}
        <div
          class="absolute top-4 right-4 bottom-4 flex w-[28%] items-center justify-center rounded-lg border text-[0.72rem] tracking-wide uppercase
						{dropIntent === 'right' ? 'border-accent bg-accent/14 text-accent' : 'border-edge/70 bg-surface/55 text-muted'}"
        >
          Split Right
        </div>
      {/if}

      {#if canSplitDown}
        <div
          class="absolute right-4 bottom-4 left-4 flex h-[28%] items-center justify-center rounded-lg border text-[0.72rem] tracking-wide uppercase
						{dropIntent === 'down' ? 'border-accent bg-accent/14 text-accent' : 'border-edge/70 bg-surface/55 text-muted'}"
        >
          Split Down
        </div>
      {/if}
    </div>
  {/if}
</div>
