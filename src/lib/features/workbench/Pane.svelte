<script lang="ts">
  import type { PaneState, Tab } from '$lib/features/workbench/model'
  import { setFocusedPane, getFocusedPaneSlot } from '$lib/features/workbench/state.svelte'
  import { paneDndUi, paneDropObserver } from '$lib/features/dnd/adapters/pane.svelte'
  import DropOverlay from '$lib/features/dnd/DropOverlay.svelte'
  import { LocalTransfer } from '$lib/features/dnd'
  import PaneTabBar from '$lib/features/workbench/PaneTabBar.svelte'
  import SurfaceHost from '$lib/features/workbench/SurfaceHost.svelte'
  import { getSessions, updateSessionInList } from '$lib/features/sessions/state/sessions.svelte'
  import { refreshGit } from '$lib/features/git/state.svelte'
  import { closeLauncherSurface, openLauncherSurface } from '$lib/features/workbench/surfaces/launcher/service.svelte'

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

  let sessions = $derived(getSessions())
  let activeTab = $derived(pane.activeTabId ? (tabs.find((tab) => tab.id === pane.activeTabId) ?? null) : null)
  let paneSession = $derived(
    activeTab?.kind === 'session' ? (sessions.find((session) => session.id === activeTab.sessionId) ?? null) : null
  )

  // Only content tabs get the lock overlay. Launcher tabs never carry content
  // worth shielding, and rendering the overlay on top of the picker would
  // confuse the user — they can always click the lock icon to unlock.
  let showLockedOverlay = $derived(activeTab !== null && activeTab.kind !== 'launcher' && activeTab.locked)
  let dropVisible = $derived(paneDndUi.visible(projectId, pane.slot))
  let dropZone = $derived(paneDndUi.zone(projectId, pane.slot))
  let dropLabel = $derived(paneDndUi.label(projectId, pane.slot))

  // Shields the pane content area during an internal drag so Monaco / xterm
  // can't swallow the drop. Session panes keep the shield off for file-like
  // drags so the terminal can still insert paths at the prompt.
  let activeDrag = $derived(LocalTransfer.peek())
  let dragHasTab = $derived(activeDrag?.items.some((item) => item.kind === 'tab') ?? false)
  let contentShielded = $derived.by(() => {
    if (!activeDrag) return false
    if (activeTab?.kind === 'session') return dragHasTab
    return true
  })

  let paneRootEl: HTMLDivElement | undefined = $state()

  function handleFocus() {
    setFocusedPane(pane.slot)
  }

  function handleNewSession() {
    handleFocus()
    // Pass the pane slot explicitly: focus is set above but the commit
    // ordering inside the store means the "active pane" lookup could
    // still see the previous focus if two panes race for a + click.
    openLauncherSurface(projectId, pane.slot)
  }

  function handleSessionCreated() {
    // The picker did its job — drop the launcher tab so the user isn't left
    // with a stale "New" tab next to the session they just started. If
    // they deliberately locked the launcher tab, respect that (closeTab skips
    // locked) and leave it.
    closeLauncherSurface(projectId, pane.slot)
  }

  // When a tab is locked we want *all* content-area interaction to stop —
  // pointer events are blocked by the overlay itself, but keyboard focus
  // may still be sitting inside Monaco or xterm, so without blurring the
  // user can keep typing through the lock. This was the Monaco bug:
  // SessionTerminal disabled input at the manager layer; Monaco had no
  // equivalent and quietly kept accepting keystrokes.
  $effect(() => {
    if (!showLockedOverlay) return
    const active = document.activeElement
    if (active instanceof HTMLElement && paneRootEl?.contains(active)) {
      active.blur()
    }
  })
</script>

<div
  bind:this={paneRootEl}
  class="relative flex h-full min-h-0 w-full min-w-0 flex-col overflow-hidden"
  onfocusin={handleFocus}
  onpointerdown={handleFocus}
  {@attach paneDropObserver({
    pane,
    projectId,
    projectPath,
    locked: showLockedOverlay
  })}
  role="region"
>
  <PaneTabBar
    {tabs}
    activeTabId={pane.activeTabId}
    paneSlot={pane.slot}
    {projectId}
    {isFocused}
    onNewSession={handleNewSession}
  />

  <div class="relative flex min-h-0 flex-1 flex-col overflow-hidden">
    <SurfaceHost
      {activeTab}
      {paneSession}
      {projectId}
      {projectPath}
      onSessionCreated={handleSessionCreated}
      onSessionStatusChange={(status) => {
        if (!paneSession) return
        updateSessionInList(paneSession.id, { status })
        if (status === 'exited' || status === 'stopped') {
          void refreshGit(projectId, projectPath)
        }
      }}
    />

    {#if showLockedOverlay}
      <!-- pointer-events-auto (default) is deliberate: the overlay has to
           actually intercept clicks, otherwise they fall through to Monaco
           / xterm / etc. and the lock is visual-only. This was bug #1
           (Monaco ignoring lock) — each tab type implemented its own lock
           and Monaco simply didn't. Combined with the focus-blur $effect
           above, this gates both pointer and keyboard input at the pane
           boundary regardless of tab kind. -->
      <div
        class="absolute inset-0 z-10 flex items-center justify-center bg-ground/72 backdrop-blur-[1px]"
        role="presentation"
      >
        <div
          class="rounded-lg border border-edge bg-surface/95 px-3 py-2 text-center text-sm text-muted shadow-popover"
        >
          <div class="font-medium text-fg">Tab locked</div>
          <div>Unlock it from the tab menu to interact.</div>
        </div>
      </div>
    {/if}

    {#if contentShielded}
      <!-- Transparent shield so drag events hit the pane observer instead
           of being absorbed by Monaco's or xterm's own DOM handlers. -->
      <div class="absolute inset-0 z-20" aria-hidden="true"></div>
    {/if}
  </div>

  <DropOverlay visible={dropVisible} zone={dropZone} label={dropLabel} />
</div>
