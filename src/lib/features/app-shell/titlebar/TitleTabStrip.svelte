<!--
  @component
  TitleTabStrip — the single global tab list in the title bar. Tabs from
  every folder sit side by side; the active one drives the sidebar and
  status bar. Click to activate, middle-click to close, drag to reorder.
-->

<script lang="ts">
  import { platform } from '$lib/platform'
  import {
    confirmCloseWorkbench,
    takeOverServerWorkbench,
    moveServerWorkbenchToNewWindow,
    removeServerWorkbenchFromWindow
  } from '$lib/features/app-actions/actions.svelte'
  import { TabButton, TabStrip } from '$lib/components/ui/chrome-tabs'
  import {
    ContextMenuRoot,
    ContextMenuTrigger,
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuSeparator
  } from '$lib/components/ui/context-menu'
  import {
    DropdownMenuRoot,
    DropdownMenuTrigger,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator
  } from '$lib/components/ui/dropdown-menu'
  import {
    canLockTab,
    isProcessLive,
    tabServer,
    type SessionTab,
    type Tab,
    type TabId
  } from '$lib/features/workbench/model'
  import { canMoveGroupAt, canReorderAt } from '$lib/features/workbench/tabInsertion'
  import {
    focusGroup,
    getGroups,
    getTabGroup,
    moveGroup,
    isTabInert,
    retryGroup,
    closeGroupWorkbench,
    type WorkbenchGroup
  } from '$lib/features/workbench/groups.svelte'
  import {
    getActiveTabId,
    getTabs,
    promoteTab,
    reorderTab,
    setActiveTab,
    toggleTabLocked
  } from '$lib/features/workbench/state.svelte'
  import { DND_MIME } from '$lib/features/dnd/payload'
  import { groupDragSource, tabDragSource } from '$lib/features/dnd/adapters/tab-strip'
  import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
  import { dropFromOtherWindow, isTabTransferring } from '$lib/features/workbench/transferService.svelte'
  import { restartSessionProcess, stopSessionProcess } from '$lib/features/sessions/service.svelte'
  import ProviderIcon from '$lib/features/sessions/providers/ProviderIcon.svelte'
  import { closeTabWithChecks } from '$lib/features/workbench/tabActions.svelte'
  import { findTask } from '$lib/features/tasks/state.svelte'
  import { openTaskTab, stopTaskProcess } from '$lib/features/tasks/service.svelte'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { FileDiff, Layers, Lock, Plus, CircleDot, TerminalIcon, ServerIcon } from '$lib/icons/lucideExports'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import LucideIcon from '$lib/icons/LucideIcon.svelte'
  import { onMount, tick } from 'svelte'
  import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
  import { getTabPresentation } from '$lib/features/workbench/presentation.svelte'
  import { getSettings } from '$lib/features/settings/state/settings.svelte'
  import { getPathColor } from '$lib/utils/pathColor'
  import NewTabButton from './NewTabButton.svelte'

  let tabs = $derived(getTabs())
  let blocks = $derived.by(() => {
    const result: { group: WorkbenchGroup | null; entries: { tab: Tab; index: number }[] }[] = []
    tabs.forEach((tab, index) => {
      const group = getTabGroup(tab)
      const last = result[result.length - 1]
      if (last && last.group?.server === group?.server) last.entries.push({ tab, index })
      else result.push({ group, entries: [{ tab, index }] })
    })
    // Workbenches controlled elsewhere keep only their chip, trailing the strip where Take Back appends.
    for (const group of getGroups()) {
      if (!result.some((block) => block.group?.server === group.server)) result.push({ group, entries: [] })
    }
    return result
  })
  let activeTabId = $derived(getActiveTabId())
  let settings = $derived(getSettings())
  let beamPosition = $derived(settings?.window.tab_beam_position ?? 'top')
  let stripEl = $state<HTMLElement | null>(null)
  let seamX = $state(2)

  // Keep the active tab in view when it changes (new tab appended off-screen,
  // Ctrl+Tab-style activation of a far tab).
  $effect(() => {
    const id = activeTabId
    if (!id || !stripEl) return
    void tick().then(() => {
      stripEl?.querySelector<HTMLElement>(`[data-tab-id="${CSS.escape(id)}"]`)?.scrollIntoView({ inline: 'nearest' })
    })
  })
  onMount(() => {
    window.addEventListener('dragend', clearDropTarget)
    window.addEventListener('drop', clearDropTarget)
    window.addEventListener('blur', clearDropTarget)
    return () => {
      window.removeEventListener('dragend', clearDropTarget)
      window.removeEventListener('drop', clearDropTarget)
      window.removeEventListener('blur', clearDropTarget)
    }
  })

  async function handleTabClose(e: Event, tabId: TabId) {
    e.stopPropagation()
    await closeTabWithChecks(tabId)
  }

  function handleAuxClick(e: MouseEvent, tabId: TabId) {
    if (e.button !== 1) return
    void handleTabClose(e, tabId)
  }

  // DRAG REORDER //
  // Tabs and server tabs are the drop targets: a tab's hovered half picks the insertion slot,
  // a server tab always means its group's leading slot.
  let dropIndex = $state<number | null>(null)
  // Another window's payload is unreadable until drop; its types still say whether it's a group.
  let foreignGroup = $state(false)

  // Fixed for the whole drag; derive once instead of rescanning on every dragover.
  let dragFrom = $derived.by(() => {
    const item = LocalTransfer.peek()?.items[0]
    return item?.kind === 'tab' ? tabs.findIndex((t) => t.id === item.tabId) : -1
  })
  let draggedGroup = $derived.by(() => {
    const item = LocalTransfer.peek()?.items[0]
    return item?.kind === 'workbench' ? item.server : null
  })
  let showDropSeam = $derived(dropIndex !== null && canDropAt(dropIndex))

  function canDropAt(slot: number): boolean {
    if (draggedGroup !== null) return canMoveGroupAt(tabs, slot, draggedGroup)
    if (dragFrom >= 0) return canReorderAt(tabs, dragFrom, slot)
    return !foreignGroup || canMoveGroupAt(tabs, slot)
  }

  function isTabDrag(e: DragEvent) {
    const types = e.dataTransfer?.types ?? []
    return (
      dragFrom >= 0 ||
      draggedGroup !== null ||
      (platform.native !== null &&
        (types.includes(DND_MIME.SWORM_TAB) || types.includes(DND_MIME.SWORM_WORKBENCH)))
    )
  }

  function clearDropTarget() {
    dropIndex = null
  }

  function clampSeamX(x: number) {
    const scroller = stripEl?.querySelector<HTMLElement>('[role="tablist"]')
    if (!scroller) return x
    const left = scroller.scrollLeft + 2
    const right = scroller.scrollLeft + scroller.clientWidth - 2
    return Math.min(Math.max(x, left), right)
  }

  /** A group's leading slot sits before its server tab, unless the dragged tab belongs to that group. */
  function slotX(slot: number): number {
    const find = (selector: string) => stripEl?.querySelector<HTMLElement>(selector)
    const right = tabs[slot]
    const server = right ? tabServer(right) : null
    const mover = dragFrom >= 0 ? tabServer(tabs[dragFrom]) : null
    if (server !== null && server !== mover && (slot === 0 || tabServer(tabs[slot - 1]) !== server))
      return find(`[data-tab-group="${CSS.escape(server)}"]`)?.offsetLeft ?? 0
    if (right) return find(`[data-tab-id="${CSS.escape(right.id)}"]`)?.offsetLeft ?? 0
    const last = slot > 0 ? find(`[data-tab-id="${CSS.escape(tabs[slot - 1].id)}"]`) : null
    return last ? last.offsetLeft + last.offsetWidth : 0
  }

  function setDropSlot(e: DragEvent, slot: number) {
    e.preventDefault()
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
    foreignGroup = Boolean(e.dataTransfer?.types.includes(DND_MIME.SWORM_WORKBENCH))
    dropIndex = slot
    seamX = clampSeamX(slotX(slot))
  }

  function handleDragOver(e: DragEvent, index: number) {
    if (!isTabDrag(e)) return
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    setDropSlot(e, e.clientX < rect.left + rect.width / 2 ? index : index + 1)
  }

  function handleGroupDragOver(e: DragEvent, server: string) {
    const first = tabs.findIndex((tab) => tabServer(tab) === server)
    if (first >= 0 && isTabDrag(e)) setDropSlot(e, first)
  }

  function handleStripDragOver(e: DragEvent) {
    // A tab or server tab below already claimed this dragover.
    if (e.defaultPrevented || !isTabDrag(e)) return
    setDropSlot(e, tabs.length)
  }

  function handleStripDragLeave(e: DragEvent) {
    const relatedTarget = e.relatedTarget
    if (relatedTarget instanceof Node && (e.currentTarget as HTMLElement).contains(relatedTarget)) return
    clearDropTarget()
  }

  function handleDrop(e: DragEvent) {
    const slot = dropIndex
    const allowed = slot !== null && canDropAt(slot)
    if (dragFrom >= 0 || draggedGroup !== null) {
      if (slot === null) return
      e.preventDefault()
      if (draggedGroup !== null) {
        if (allowed) moveGroup(draggedGroup, slot)
      } else {
        // Insertion slot → post-removal index.
        reorderTab(dragFrom, slot > dragFrom ? slot - 1 : slot)
      }
      clearDropTarget()
      LocalTransfer.clear()
      return
    }

    if (platform.native && (slot === null || allowed)) dropFromOtherWindow(e, slot ?? tabs.length)
    clearDropTarget()
  }

  async function confirmCloseGroup(server: string): Promise<void> {
    const folders = tabs.filter((tab) => tabServer(tab) === server).map((tab) => tab.folderPath)
    if (await confirmCloseWorkbench(folders)) {
      await runNotifiedTask(() => closeGroupWorkbench(server), {
        loading: { title: 'Closing workbench', description: server },
        error: { title: 'Close workbench failed' }
      })
    }
  }

  // SESSION MENU //
  async function stopSession(tab: SessionTab) {
    await runNotifiedTask(() => stopSessionProcess(tab.id), {
      loading: { title: 'Stopping session', description: tab.title },
      success: { title: 'Session stopped', description: tab.title },
      error: { title: 'Stop session failed' }
    })
  }

  async function restartSession(tab: SessionTab) {
    setActiveTab(tab.id)
    await tick()
    await runNotifiedTask(() => restartSessionProcess(tab), {
      loading: { title: 'Restarting session', description: tab.title },
      success: { title: 'Session restarted', description: tab.title },
      error: { title: 'Restart session failed' }
    })
  }

  // TASK MENU //
  async function handleTaskStop(tab: Tab) {
    if (tab.kind !== 'task') return
    await runNotifiedTask(
      () => stopTaskProcess(tab),
      {
        loading: { title: 'Stopping task', description: tab.label },
        error: { title: 'Stop task failed' }
      }
    )
  }

  async function handleTaskRestart(tab: Tab) {
    if (tab.kind !== 'task') return
    const def = findTask(tab.folderPath, tab.taskId)
    if (!def) {
      notify.error('Cannot restart task', `Task "${tab.taskId}" is no longer defined in .sworm/tasks.jsonc`)
      return
    }
    await openTaskTab(tab.folderPath, def, { activeFilePath: tab.activeFilePath })
  }
</script>

{#snippet tabItem(tab: Tab, i: number)}
  {@const presentation = getTabPresentation(tab)}
  {@const sessionLive = tab.kind === 'session' && isProcessLive(tab.status)}
  {@const transferring = isTabTransferring(tab.id)}
  {@const tabColor = getPathColor(tab.folderPath)}
  {@const inert = isTabInert(tab)}
  <ContextMenuRoot>
    <ContextMenuTrigger
      class="contents"
      disabled={inert}
      draggable={!tab.locked && !transferring && !inert}
      {@attach transferring || inert ? undefined : tabDragSource({ tab })}
    >
      <TabButton
        active={activeTabId === tab.id}
        position={beamPosition}
        color={tabColor}
        class={dragFrom === i ? 'opacity-40' : undefined}
        draggable={!tab.locked && !transferring && !inert}
        data-tab-id={tab.id}
        title="{tab.folderPath} — {presentation.title}"
        onclick={() => setActiveTab(tab.id)}
        ondblclick={() => {
          if (tab.kind !== 'new-tab' && presentation.preview) {
            promoteTab(tab.id)
          }
        }}
        onauxclick={tab.locked || transferring || inert ? undefined : (e) => handleAuxClick(e, tab.id)}
        ondragover={(e) => handleDragOver(e, i)}
        ondragend={clearDropTarget}
        onClose={tab.locked || transferring || inert ? undefined : (e) => handleTabClose(e, tab.id)}
      >
        {#snippet leading()}
          {#if tab.kind === 'diff'}
            <FileDiff size={14} class="shrink-0 text-accent" />
          {:else if tab.kind === 'issue'}
            <CircleDot size={14} class="shrink-0 text-accent" />
          {:else if tab.kind === 'epic'}
            <Layers size={14} class="shrink-0 text-warning" />
          {:else if tab.kind === 'text' && presentation.fileName}
            <!-- Pass the full relative path so the resolver can apply
                     directory-aware rules (e.g. .sworm/*.json → sworm icon).
                     Falls back to the basename for unsaved "Untitled" tabs. -->
            <FileIcon filename={tab.filePath ?? presentation.fileName} size={14} />
          {:else if tab.kind === 'new-tab'}
            <Plus size={14} class="shrink-0 text-accent" />
          {:else if tab.kind === 'task'}
            <!-- Task icon comes from .sworm/tasks.jsonc. Any Lucide name
                     is valid; fall back to the terminal glyph when the
                     dynamic loader can't find a match. -->
            {#if presentation.lucideIcon}
              <LucideIcon name={presentation.lucideIcon} size={14} class="shrink-0 text-accent" />
            {:else}
              <TerminalIcon size={14} class="shrink-0 text-accent" />
            {/if}
          {:else if presentation.providerIcon}
            <ProviderIcon icon={presentation.providerIcon} size={14} />
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
      {#if tab.kind === 'session'}
        <ContextMenuItem onclick={() => void (sessionLive ? stopSession(tab) : restartSession(tab))}>
          {sessionLive ? 'Stop' : 'Restart'}
        </ContextMenuItem>
      {/if}
      {#if tab.kind === 'task'}
        {@const taskRunning = tab.status === 'running' || tab.status === 'starting'}
        <ContextMenuItem onclick={() => void (taskRunning ? handleTaskStop(tab) : handleTaskRestart(tab))}>
          {taskRunning ? 'Stop' : 'Restart'}
        </ContextMenuItem>
      {/if}
      {#if canLockTab(tab)}
        <!-- Lock only makes sense on content tabs where accidental input
                 can cause damage (session terminals, Monaco text tabs). New tab pages
                 and diff tabs skip this affordance entirely. -->
        <ContextMenuItem onclick={() => toggleTabLocked(tab.id)}>
          {tab.locked ? 'Unlock Tab' : 'Lock Tab'}
        </ContextMenuItem>
        <ContextMenuSeparator />
      {/if}
      <ContextMenuItem
        destructive
        disabled={tab.locked || transferring}
        onclick={() => void closeTabWithChecks(tab.id)}
      >
        Close
      </ContextMenuItem>
    </ContextMenuContent>
  </ContextMenuRoot>
{/snippet}

{#snippet groupLabel(group: WorkbenchGroup)}
  <ServerIcon size={14} class="shrink-0" />
  <span class="truncate">{group.server}</span>
  {#if group.state === 'busy' || group.state === 'revoked'}
    <span class="max-w-28 shrink-0 truncate font-normal">in {group.client ?? 'another app'}</span>
  {/if}
{/snippet}

{#snippet groupActions(group: WorkbenchGroup, Item: typeof ContextMenuItem, Separator: typeof ContextMenuSeparator)}
  {#if group.state === 'busy' || group.state === 'revoked'}
    <Item onclick={() => void takeOverServerWorkbench(group.server)}>Take Back</Item>
  {:else if group.state === 'offline'}
    <Item onclick={() => void retryGroup(group.server)}>Retry</Item>
  {:else if group.state === 'active'}
    <Item onclick={() => void moveServerWorkbenchToNewWindow(group.server)}>Move to New Window</Item>
  {/if}
  {#if group.state !== 'active'}
    <Item onclick={() => void removeServerWorkbenchFromWindow(group.server)}>Remove from Window</Item>
  {/if}
  <Separator />
  <Item destructive onclick={() => void confirmCloseGroup(group.server)}>Close Workbench…</Item>
{/snippet}

<div
  class="flex min-w-0 flex-1 self-stretch {dropIndex !== null && dragFrom < 0 && draggedGroup === null
    ? 'bg-accent/10'
    : ''}"
  role="group"
  aria-label="Tabs and tab drop area"
  ondragover={handleStripDragOver}
  ondragleave={handleStripDragLeave}
  ondrop={handleDrop}
>
  <div class="flex min-w-0 shrink self-stretch" bind:this={stripEl}>
    <TabStrip ariaLabel="Tabs" class="h-full">
      {#each blocks as block (block.group?.server ?? block.entries[0].tab.id)}
        {#if block.group}
          {@const group = block.group}
          {@const controlled = group.state === 'active'}
          {@const serverTabClass =
            'flex h-full max-w-48 shrink-0 cursor-pointer items-center gap-1.5 bg-(--group) px-3 text-sm font-medium text-ground focus-visible:shadow-focus-ring focus-visible:outline-none'}
          <!-- The group line owns the beam edge; the border pushes each tab's own beam just inside
               it. Unpositioned: the drop seam measures tab offsets against the strip. -->
          <div
            role="group"
            aria-label="{group.server} workbench"
            data-tab-group={group.server}
            class="flex h-full shrink-0 items-center border-(--group) {beamPosition === 'bottom'
              ? 'border-b-2'
              : 'border-t-2'} {draggedGroup === group.server ? 'opacity-40' : controlled ? '' : 'opacity-60'}"
            style:--group={getPathColor(group.server)}
          >
            {#if block.entries.length}
              <!-- Acts like a tab: click shows the group, right-click for actions, drag moves the whole group. -->
              <ContextMenuRoot>
                <ContextMenuTrigger
                  class="contents"
                  {@attach controlled ? groupDragSource({ server: group.server, workbenchId: group.id }) : undefined}
                >
                  <button
                    type="button"
                    class={serverTabClass}
                    title="{group.server}: {group.state}"
                    draggable={controlled}
                    onclick={() => focusGroup(group.server)}
                    ondragover={(e) => handleGroupDragOver(e, group.server)}
                  >
                    {@render groupLabel(group)}
                  </button>
                </ContextMenuTrigger>
                <ContextMenuContent>
                  {@render groupActions(group, ContextMenuItem, ContextMenuSeparator)}
                </ContextMenuContent>
              </ContextMenuRoot>
            {:else}
              <!-- Controlled elsewhere: nothing to show or move, so a click opens the actions. -->
              <DropdownMenuRoot>
                <DropdownMenuTrigger
                  aria-label="Workbench {group.server} actions"
                  title="{group.server}: {group.state}"
                  class={serverTabClass}
                >
                  {@render groupLabel(group)}
                </DropdownMenuTrigger>
                <DropdownMenuContent align="start" sideOffset={4}>
                  {@render groupActions(group, DropdownMenuItem, DropdownMenuSeparator)}
                </DropdownMenuContent>
              </DropdownMenuRoot>
            {/if}
            {#each block.entries as { tab, index } (tab.id)}
              {@render tabItem(tab, index)}
            {/each}
          </div>
        {:else}
          {#each block.entries as { tab, index } (tab.id)}
            {@render tabItem(tab, index)}
          {/each}
        {/if}
      {/each}
      {#if showDropSeam}
        <span
          aria-hidden="true"
          style="left: {seamX}px"
          class="pointer-events-none absolute inset-y-0 z-10 w-[3px] -translate-x-1/2 rounded-full bg-accent before:absolute before:top-0 before:left-1/2 before:h-[3px] before:w-[9px] before:-translate-x-1/2 before:rounded-[1px] before:bg-accent before:content-[''] after:absolute after:bottom-0 after:left-1/2 after:h-[3px] after:w-[9px] after:-translate-x-1/2 after:rounded-[1px] after:bg-accent after:content-['']"
        ></span>
      {/if}

      {#snippet trailing()}
        <NewTabButton />
      {/snippet}
    </TabStrip>
  </div>

  <!-- Keep trailing title-bar space both draggable and a tab drop target. -->
  <div
    data-tauri-drag-region={platform.native ? '' : undefined}
    class="min-w-0 flex-1 self-stretch"
  ></div>
</div>
