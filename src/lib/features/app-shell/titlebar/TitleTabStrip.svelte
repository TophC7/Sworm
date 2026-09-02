<!--
  @component
  TitleTabStrip — the single global tab list in the title bar. Tabs from
  every folder sit side by side; the active one drives the sidebar and
  status bar. Click to activate, middle-click to close, drag to reorder.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { TabButton, TabStrip } from '$lib/components/ui/chrome-tabs'
  import {
    ContextMenuRoot,
    ContextMenuTrigger,
    ContextMenuContent,
    ContextMenuItem,
    ContextMenuSeparator
  } from '$lib/components/ui/context-menu'
  import { canLockTab, isProcessLive, type SessionTab, type Tab, type TabId } from '$lib/features/workbench/model'
  import {
    getActiveTabId,
    getTabs,
    promoteTab,
    reorderTab,
    setActiveTab,
    setTaskTabStatus,
    toggleTabLocked
  } from '$lib/features/workbench/state.svelte'
  import { tabDragSource } from '$lib/features/dnd/adapters/tab-strip'
  import { LocalTransfer } from '$lib/features/dnd/transfer.svelte'
  import { startSessionProcess, stopSessionProcess } from '$lib/features/sessions/service.svelte'
  import * as sessionRegistry from '$lib/features/sessions/terminal/sessionRegistry'
  import { sessionDotClass } from '$lib/features/sessions/visual'
  import * as taskRegistry from '$lib/features/tasks/taskRegistry'
  import { closeTabWithChecks } from '$lib/features/workbench/tabActions.svelte'
  import { findTask } from '$lib/features/tasks/state.svelte'
  import { openTaskTab } from '$lib/features/tasks/service.svelte'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { FileDiff, BellIcon, Layers, Lock, Plus, CircleDot, TerminalIcon } from '$lib/icons/lucideExports'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import LucideIcon from '$lib/icons/LucideIcon.svelte'
  import { tick } from 'svelte'
  import { runNotifiedTask } from '$lib/features/notifications/runNotifiedTask'
  import { getTabPresentation } from '$lib/features/workbench/presentation.svelte'
  import { getSurfaceKind } from '$lib/features/workbench/surfaces'
  import { cn } from '$lib/utils/cn'
  import NewTabMenu from './NewTabMenu.svelte'

  let tabs = $derived(getTabs())
  let activeTabId = $derived(getActiveTabId())
  let stripEl = $state<HTMLElement | null>(null)

  // Keep the active tab in view when it changes (new tab appended off-screen,
  // Ctrl+Tab-style activation of a far tab).
  $effect(() => {
    const id = activeTabId
    if (!id || !stripEl) return
    void tick().then(() => {
      stripEl?.querySelector<HTMLElement>(`[data-tab-id="${CSS.escape(id)}"]`)?.scrollIntoView({ inline: 'nearest' })
    })
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
  // Drop targets are the tabs themselves; the left/right half of the
  // hovered tab decides the insertion slot.
  let dropIndex = $state<number | null>(null)

  // The source index is fixed for the whole drag; derive it once instead
  // of rescanning the transfer and tab list on every dragover.
  let dragFrom = $derived.by(() => {
    const item = LocalTransfer.peek()?.items.find((i) => i.kind === 'tab')
    return item ? tabs.findIndex((t) => t.id === item.tabId) : -1
  })

  // The source adapter clears LocalTransfer on dragend/drop; piggyback on
  // that to drop the insertion marker when a drag ends anywhere.
  $effect(() => {
    if (dragFrom < 0) dropIndex = null
  })

  function handleDragOver(e: DragEvent, index: number) {
    if (dragFrom < 0) return
    e.preventDefault()
    if (e.dataTransfer) e.dataTransfer.dropEffect = 'move'
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    dropIndex = e.clientX < rect.left + rect.width / 2 ? index : index + 1
  }

  function handleDrop(e: DragEvent) {
    const from = dragFrom
    if (from < 0 || dropIndex === null) return
    e.preventDefault()
    // Insertion slot → post-removal index.
    const to = dropIndex > from ? dropIndex - 1 : dropIndex
    reorderTab(from, to)
    dropIndex = null
    LocalTransfer.clear()
  }

  // SESSION MENU //
  async function stopSession(tab: SessionTab) {
    await runNotifiedTask(() => stopSessionProcess(tab.sessionId), {
      loading: { title: 'Stopping session', description: tab.title },
      success: { title: 'Session stopped', description: tab.title },
      error: { title: 'Stop session failed' }
    })
  }

  async function restartSession(tab: SessionTab) {
    setActiveTab(tab.id)
    await tick()
    await runNotifiedTask(
      async () => {
        const manager = sessionRegistry.getOrCreate(tab.sessionId)
        if (manager.isPtyActive()) await manager.stopPty()
        await startSessionProcess(manager, tab)
      },
      {
        loading: { title: 'Restarting session', description: tab.title },
        success: { title: 'Session restarted', description: tab.title },
        error: { title: 'Restart session failed' }
      }
    )
  }

  // TASK MENU //
  async function handleTaskStop(tab: Tab) {
    if (tab.kind !== 'task') return
    const manager = taskRegistry.get(tab.runId)
    await runNotifiedTask(
      async () => {
        if (manager) {
          await manager.stopProcess()
          return
        }
        await backend.tasks.stop(tab.runId)
        setTaskTabStatus(tab.id, 'exited', null)
      },
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
      notify.error('Cannot restart task', `Task "${tab.taskId}" is no longer defined in .sworm/tasks.json`)
      return
    }
    await openTaskTab(tab.folderPath, def, { activeFilePath: tab.activeFilePath })
  }
</script>

<div class="flex min-w-0 shrink self-stretch" bind:this={stripEl}>
  <TabStrip ariaLabel="Tabs" class="h-full">
    {#each tabs as tab, i (tab.id)}
      {@const presentation = getTabPresentation(tab)}
      {@const surfaceKind = getSurfaceKind(tab)}
      {@const sessionLive = tab.kind === 'session' && isProcessLive(tab.status)}
      <ContextMenuRoot>
        <ContextMenuTrigger
          class={cn(
            'contents',
            dropIndex === i && '[&>button]:shadow-[inset_2px_0_0_var(--color-accent)]',
            dropIndex === i + 1 && i === tabs.length - 1 && '[&>button]:shadow-[inset_-2px_0_0_var(--color-accent)]'
          )}
          draggable={!tab.locked}
          {@attach tabDragSource({ tab })}
        >
          <TabButton
            active={activeTabId === tab.id}
            draggable={!tab.locked}
            data-tab-id={tab.id}
            title="{tab.folderPath} — {presentation.title}"
            onclick={() => setActiveTab(tab.id)}
            ondblclick={() => {
              if (surfaceKind !== 'session' && surfaceKind !== 'launcher' && presentation.preview) {
                promoteTab(tab.id)
              }
            }}
            onauxclick={(e) => handleAuxClick(e, tab.id)}
            ondragover={(e) => handleDragOver(e, i)}
            ondrop={handleDrop}
            onClose={tab.locked ? undefined : (e) => handleTabClose(e, tab.id)}
          >
            {#snippet leading()}
              {#if surfaceKind === 'diff'}
                <FileDiff size={14} class="shrink-0 text-accent" />
              {:else if surfaceKind === 'tool'}
                <BellIcon size={14} class="shrink-0 text-accent" />
              {:else if surfaceKind === 'issue'}
                <CircleDot size={14} class="shrink-0 text-accent" />
              {:else if surfaceKind === 'epic'}
                <Layers size={14} class="shrink-0 text-warning" />
              {:else if tab.kind === 'text' && presentation.fileName}
                <!-- Pass the full relative path so the resolver can apply
                     directory-aware rules (e.g. .sworm/*.json → sworm icon).
                     Falls back to the basename for unsaved "Untitled" tabs. -->
                <FileIcon filename={tab.filePath ?? presentation.fileName} size={14} />
              {:else if surfaceKind === 'launcher'}
                <Plus size={14} class="shrink-0 text-accent" />
              {:else if surfaceKind === 'task'}
                <!-- Task icon comes from .sworm/tasks.json. Any Lucide name
                     is valid; fall back to the terminal glyph when the
                     dynamic loader can't find a match. -->
                {#if presentation.lucideIcon}
                  <LucideIcon name={presentation.lucideIcon} size={14} class="shrink-0 text-accent" />
                {:else}
                  <TerminalIcon size={14} class="shrink-0 text-accent" />
                {/if}
              {:else if presentation.providerIcon}
                <img src={presentation.providerIcon} alt="" width={14} height={14} class="shrink-0" />
              {/if}
              {#if tab.kind === 'session'}
                <span class="h-1.5 w-1.5 shrink-0 rounded-full {sessionDotClass(tab)}" aria-label={tab.status}></span>
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
                 can cause damage (session terminals, Monaco text tabs). Launcher
                 and diff tabs skip this affordance entirely. -->
            <ContextMenuItem onclick={() => toggleTabLocked(tab.id)}>
              {tab.locked ? 'Unlock Tab' : 'Lock Tab'}
            </ContextMenuItem>
            <ContextMenuSeparator />
          {/if}
          <ContextMenuItem destructive disabled={tab.locked} onclick={() => void closeTabWithChecks(tab.id)}>
            Close
          </ContextMenuItem>
        </ContextMenuContent>
      </ContextMenuRoot>
    {/each}

    {#snippet trailing()}
      <NewTabMenu />
    {/snippet}
  </TabStrip>
</div>
