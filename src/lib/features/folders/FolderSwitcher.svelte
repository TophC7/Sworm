<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { backend } from '$lib/api/backend'
  import {
    BreadcrumbItem,
    BreadcrumbLink,
    BreadcrumbList,
    BreadcrumbPage,
    BreadcrumbRoot,
    BreadcrumbSeparator
  } from '$lib/components/ui/breadcrumb'
  import { Button, IconButton } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { getRecentFolders } from '$lib/features/folders/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { getActiveFolderPath, openFolder } from '$lib/features/workbench/state.svelte'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import {
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ChevronRight,
    Eye,
    EyeOff,
    FolderOpen
  } from '$lib/icons/lucideExports'
  import type { FolderEntry } from '$lib/types/backend'
  import { logicalKey } from '$lib/utils/keyboardEvent'
  import { basename, dirname, pathCrumbs } from '$lib/utils/paths'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'
  import { isFolderSwitcherOpen, setFolderSwitcherOpen } from './switcher.svelte'
  let surfaceRef = $state<HTMLDivElement | null>(null)
  let inputRef = $state<HTMLInputElement | null>(null)
  let containerPath = $state('/')
  let selectedPath = $state<string | null>(null)
  let showHidden = $state(false)
  let filterQuery = $state('')
  let containerEntries = $state<FolderEntry[]>([])
  let containerEntriesPath = $state<string | null>(null)
  let containerError = $state<string | null>(null)
  let selectedEntries = $state<FolderEntry[]>([])
  let selectedEntriesPath = $state<string | null>(null)
  let selectedError = $state<string | null>(null)

  const containerLoad = createTrackedAsyncLoad<string | null>()
  const selectedLoad = createTrackedAsyncLoad<string | null>()

  let open = $derived(isFolderSwitcherOpen())
  let leftRows = $derived(
    containerEntriesPath === containerPath ? containerEntries.filter((entry) => entry.is_dir) : []
  )
  let previewEntries = $derived(selectedEntriesPath === selectedPath ? selectedEntries : [])
  let q = $derived(filterQuery.trim().toLowerCase())
  let crumbs = $derived(pathCrumbs(containerPath))
  let recents = $derived(getRecentFolders().slice(0, 6))

  function isMatch(entry: FolderEntry): boolean {
    return q.length === 0 || entry.name.toLowerCase().includes(q)
  }

  function drillInto(path: string): void {
    containerPath = path
    selectedPath = null
    filterQuery = ''
  }

  function goUp(): void {
    if (containerPath === '/') return
    const previousPath = containerPath
    containerPath = dirname(previousPath) || '/'
    selectedPath = previousPath
    filterQuery = ''
  }

  async function enterFolder(path: string): Promise<void> {
    setFolderSwitcherOpen(false)
    await openFolder(path)
  }

  function selectPreviewFolder(path: string): void {
    if (!selectedPath) return
    containerPath = selectedPath
    selectedPath = path
    filterQuery = ''
  }

  function selectCrumb(event: MouseEvent, index: number): void {
    event.preventDefault()
    const nextCrumb = crumbs[index + 1]
    containerPath = crumbs[index].path
    selectedPath = nextCrumb?.path ?? null
    filterQuery = ''
  }

  function focusSelectedFolder(force = false): void {
    void tick().then(() => {
      if (!isFolderSwitcherOpen()) return
      if (!force && document.activeElement === inputRef) return
      const selectedIndex = leftRows.findIndex((entry) => entry.path === selectedPath)
      const index = selectedIndex >= 0 ? selectedIndex : 0
      const row = surfaceRef?.querySelector<HTMLElement>(`[data-left-index="${index}"]`)
      row?.scrollIntoView({ block: 'nearest' })
      row?.querySelector<HTMLButtonElement>('button')?.focus()
    })
  }

  function moveSelection(delta: number): void {
    if (leftRows.length === 0) return
    const currentIndex = leftRows.findIndex((entry) => entry.path === selectedPath)
    const base = currentIndex >= 0 ? currentIndex : 0
    const nextIndex = Math.max(0, Math.min(leftRows.length - 1, base + delta))
    selectedPath = leftRows[nextIndex].path
    void tick().then(() => {
      const row = surfaceRef?.querySelector<HTMLElement>(`[data-left-index="${nextIndex}"]`)
      row?.scrollIntoView({ block: 'nearest' })
      row?.querySelector<HTMLButtonElement>('button')?.focus()
    })
  }

  function handleKeyDown(event: KeyboardEvent): void {
    const target = event.target
    const key = logicalKey(event)

    if (key === 'Tab' && surfaceRef) {
      const focusable = Array.from(
        surfaceRef.querySelectorAll<HTMLElement>(
          'button:not([disabled]), a[href], input:not([disabled]), [tabindex="0"]'
        )
      )
      const first = focusable[0]
      const last = focusable.at(-1)
      if (first && last) {
        if (event.shiftKey && document.activeElement === first) {
          event.preventDefault()
          last.focus()
        } else if (!event.shiftKey && document.activeElement === last) {
          event.preventDefault()
          first.focus()
        }
      }
      return
    }

    if (
      key === '/' &&
      !(
        target instanceof HTMLInputElement ||
        target instanceof HTMLTextAreaElement ||
        (target instanceof HTMLElement && target.isContentEditable)
      )
    ) {
      event.preventDefault()
      inputRef?.focus()
      return
    }

    if (
      key === 'Enter' &&
      target instanceof Element &&
      (target.closest('a[href]') || target.closest('[data-recent-chip]'))
    ) {
      return
    }

    if (key === 'ArrowDown' || key === 'ArrowUp') {
      event.preventDefault()
      moveSelection(key === 'ArrowDown' ? 1 : -1)
      return
    }

    const filterIsEditing = target === inputRef && filterQuery.length > 0
    if (key === 'ArrowRight') {
      if (filterIsEditing || !selectedPath) return
      const targetPath = selectedPath
      if (selectedEntriesPath === targetPath) {
        if (selectedEntries.some((entry) => entry.is_dir)) {
          event.preventDefault()
          drillInto(targetPath)
        }
        return
      }
      event.preventDefault()
      void (async () => {
        try {
          const entries = await backend.folders.listEntries(targetPath, showHidden)
          if (targetPath === selectedPath && entries.some((entry) => entry.is_dir)) {
            drillInto(targetPath)
          }
        } catch {
          // ignored on drill attempt
        }
      })()
      return
    }
    if (key === 'ArrowLeft') {
      if (filterIsEditing) return
      event.preventDefault()
      goUp()
      return
    }
    if (key === 'Enter' && selectedPath) {
      event.preventDefault()
      void enterFolder(selectedPath)
      return
    }
    if (key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      if (filterQuery.length > 0) filterQuery = ''
      else setFolderSwitcherOpen(false)
    }
  }

  $effect(() => {
    const opened = open
    return untrack(() => {
      if (!opened) return
      const previousFocus = document.activeElement
      const initialPath = getActiveFolderPath() ?? getRecentFolders()[0] ?? null
      if (initialPath && basename(initialPath).startsWith('.')) {
        showHidden = true
      }
      containerPath = initialPath ? dirname(initialPath) || '/' : '/'
      selectedPath = initialPath
      filterQuery = ''
      void tick().then(() => {
        if (isFolderSwitcherOpen()) inputRef?.focus()
      })
      return () => {
        void tick().then(() => {
          if (
            !isFolderSwitcherOpen() &&
            document.activeElement === document.body &&
            previousFocus instanceof HTMLElement &&
            previousFocus.isConnected
          )
            previousFocus.focus()
        })
      }
    })
  })

  $effect(() => {
    const requestedPath = containerPath
    const requestedHidden = showHidden
    const key = open ? `${requestedPath}\u0000${requestedHidden}` : null

    containerLoad.run(key, async (isCurrent) => {
      containerError = null
      containerEntries = []
      containerEntriesPath = null
      if (key === null) return

      try {
        const resolved = await backend.folders.resolve(requestedPath)
        if (!isCurrent()) return
        if (resolved.path !== requestedPath) {
          if (selectedPath?.startsWith(`${requestedPath}/`)) {
            selectedPath = resolved.path + selectedPath.slice(requestedPath.length)
          }
          containerPath = resolved.path
          return
        }

        const entries = await backend.folders.listEntries(resolved.path, requestedHidden)
        if (!isCurrent()) return
        containerEntries = entries
        containerEntriesPath = resolved.path
        const folders = entries.filter((entry) => entry.is_dir)
        const selected = folders.find((entry) => entry.path === selectedPath)
        if (!selected || !isMatch(selected)) {
          selectedPath = folders.find(isMatch)?.path ?? selected?.path ?? folders[0]?.path ?? null
        }
        focusSelectedFolder()
      } catch (cause) {
        if (!isCurrent()) return
        containerError = getErrorMessage(cause)
      }
    })
  })

  $effect(() => {
    const requestedPath = selectedPath
    const requestedHidden = showHidden
    const key = open && requestedPath ? `${requestedPath}\u0000${requestedHidden}` : null

    selectedLoad.run(key, async (isCurrent) => {
      selectedError = null
      selectedEntries = []
      selectedEntriesPath = null
      if (key === null || requestedPath === null) return

      try {
        const entries = await backend.folders.listEntries(requestedPath, requestedHidden)
        if (!isCurrent()) return
        selectedEntries = entries
        selectedEntriesPath = requestedPath
      } catch (cause) {
        if (!isCurrent()) return
        selectedError = getErrorMessage(cause)
      }
    })
  })

  $effect(() => {
    void q
    untrack(() => {
      const selected = leftRows.find((entry) => entry.path === selectedPath)
      if (selected && !isMatch(selected)) {
        selectedPath = leftRows.find(isMatch)?.path ?? selectedPath
      }
    })
  })

  $effect(() => {
    if (!open) return

    function handlePointerDown(event: PointerEvent): void {
      const target = event.target
      if (!(target instanceof Node)) return
      if (surfaceRef?.contains(target)) return
      if (target instanceof Element && target.closest('[data-folder-switcher-toggle="true"]')) return
      setFolderSwitcherOpen(false)
    }

    document.addEventListener('pointerdown', handlePointerDown)
    return () => document.removeEventListener('pointerdown', handlePointerDown)
  })
</script>

{#snippet entryRow(entry: FolderEntry, pane: 'left' | 'right', index = -1)}
  {@const selected = pane === 'left' && selectedPath === entry.path}
  {@const dimmed = pane === 'left' && !isMatch(entry)}
  <div
    class="group/tree-row relative flex w-full items-center text-sm transition-opacity {selected
      ? 'bg-accent/10 text-bright'
      : entry.is_dir
        ? 'text-muted hover:bg-surface'
        : 'text-fg'} {dimmed ? 'opacity-30' : ''}"
    style:height="22px"
    role="presentation"
    data-path={entry.path}
    data-left-index={pane === 'left' ? index : undefined}
  >
    {#if pane === 'left'}
      <button
        type="button"
        tabindex="-1"
        class="relative flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1 border-none bg-transparent py-0.5 pl-2.5 text-left text-inherit focus-visible:shadow-focus-ring focus-visible:outline-none"
        aria-pressed={selected}
        onclick={() => (selectedPath = entry.path)}
        onfocus={() => (selectedPath = entry.path)}
        ondblclick={() => void enterFolder(entry.path)}
      >
        <FileIcon filename={entry.name} folder expanded={selected} size={15} />
        <span class="truncate">{entry.name}</span>
      </button>
      <ChevronRight size={12} class="mr-2 shrink-0 text-subtle" />
    {:else if entry.is_dir}
      <button
        type="button"
        tabindex="-1"
        class="relative flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1 border-none bg-transparent py-0.5 pl-2.5 text-left text-inherit focus-visible:shadow-focus-ring focus-visible:outline-none"
        onclick={() => selectPreviewFolder(entry.path)}
      >
        <FileIcon filename={entry.name} folder expanded={false} size={15} />
        <span class="truncate">{entry.name}</span>
      </button>
      <ChevronRight size={12} class="mr-2 shrink-0 text-subtle" />
    {:else}
      <div
        class="relative flex h-full min-w-0 flex-1 items-center gap-1 bg-transparent py-0.5 pl-2.5 text-left text-inherit"
      >
        <FileIcon filename={entry.name} size={15} />
        <span class="truncate">{entry.name}</span>
      </div>
    {/if}
  </div>
{/snippet}

{#if open}
  <div class="fixed bottom-8 left-2 z-40 w-[720px] max-w-[calc(100vw-1rem)]">
    <div
      bind:this={surfaceRef}
      class="flex max-h-[calc(100vh-2.5rem)] flex-col overflow-hidden rounded-xl border border-edge bg-raised shadow-popover"
      role="dialog"
      aria-modal="true"
      aria-label="Switch Folder"
      tabindex="-1"
      onkeydown={handleKeyDown}
    >
      <div class="flex shrink-0 items-center gap-2 border-b border-edge px-3 py-2">
        <BreadcrumbRoot class="flex-1 overflow-hidden font-mono text-sm">
          <BreadcrumbList>
            {#each crumbs as crumb, index (crumb.path)}
              <BreadcrumbItem>
                {#if index === crumbs.length - 1}
                  <BreadcrumbPage>{crumb.label}</BreadcrumbPage>
                {:else}
                  <BreadcrumbLink href={crumb.path} onclick={(event) => selectCrumb(event, index)}>
                    {crumb.label}
                  </BreadcrumbLink>
                {/if}
              </BreadcrumbItem>
              {#if index < crumbs.length - 1}<BreadcrumbSeparator />{/if}
            {/each}
          </BreadcrumbList>
        </BreadcrumbRoot>
        <IconButton
          tooltip={showHidden ? 'Hide hidden folders' : 'Show hidden folders'}
          active={showHidden}
          onclick={() => {
            showHidden = !showHidden
            focusSelectedFolder(true)
          }}
        >
          {#if showHidden}<EyeOff size={12} />{:else}<Eye size={12} />{/if}
        </IconButton>
      </div>

      {#if recents.length > 0}
        <div class="flex shrink-0 flex-wrap items-center gap-1.5 border-b border-edge bg-surface px-3 py-1.5">
          <span class="text-xs font-semibold tracking-wide text-muted uppercase">Recent</span>
          {#each recents as path (path)}
            <Button variant="outline" size="xs" data-recent-chip title={path} onclick={() => void enterFolder(path)}>
              <FolderOpen size={10} />
              {basename(path) || '/'}
            </Button>
          {/each}
        </div>
      {/if}

      <div class="grid h-72 min-h-0 shrink grid-cols-2">
        <section class="flex min-h-0 flex-col border-r border-edge bg-surface" aria-label="Folders">
          <div class="section-label shrink-0 font-mono normal-case">
            {containerPath === '/' ? '/' : `${basename(containerPath)}/`}
          </div>
          <div class="min-h-0 flex-1 overflow-y-auto py-1" aria-live="polite">
            {#if containerLoad.loading && leftRows.length === 0}
              <div class="px-3 py-2 text-sm text-subtle">Loading…</div>
            {:else if containerError}
              <div class="px-3 py-2 text-sm text-danger">{containerError}</div>
            {:else if leftRows.length === 0}
              <div class="px-3 py-2 text-sm text-subtle">No folders.</div>
            {:else}
              {#each leftRows as entry, index (entry.path)}
                {@render entryRow(entry, 'left', index)}
              {/each}
            {/if}
          </div>
          <div class="shrink-0 border-t border-edge px-2 py-1.5">
            <Input
              bind:ref={inputRef}
              bind:value={filterQuery}
              placeholder="Filter folders..."
              aria-label="Filter folders"
              class="h-7 py-1 text-sm"
            />
          </div>
        </section>

        <section class="flex min-h-0 flex-col bg-ground" aria-label="Folder contents">
          <div class="section-label shrink-0 font-mono normal-case">
            {selectedPath ? (selectedPath === '/' ? '/' : `${basename(selectedPath)}/`) : 'Folder contents'}
          </div>
          <div class="min-h-0 flex-1 overflow-y-auto py-1" aria-live="polite">
            {#if !selectedPath}
              <div class="px-3 py-2 text-sm text-subtle">Select a folder.</div>
            {:else if selectedLoad.loading && previewEntries.length === 0}
              <div class="px-3 py-2 text-sm text-subtle">Loading…</div>
            {:else if selectedError}
              <div class="px-3 py-2 text-sm text-danger">{selectedError}</div>
            {:else if previewEntries.length === 0}
              <div class="px-3 py-2 text-sm text-subtle">Empty folder.</div>
            {:else}
              {#each previewEntries as entry (entry.path)}
                {@render entryRow(entry, 'right')}
              {/each}
            {/if}
          </div>
        </section>
      </div>

      <div class="flex shrink-0 items-center gap-4 border-t border-edge bg-surface px-3 py-1.5 text-xs text-subtle">
        <span class="flex items-center gap-1.5">
          <KbdGroup
            ><Kbd class="h-5 min-w-5 px-1"><ArrowUp size={10} /></Kbd><Kbd class="h-5 min-w-5 px-1"
              ><ArrowDown size={10} /></Kbd
            ></KbdGroup
          >
          Select
        </span>
        <span class="flex items-center gap-1.5">
          <KbdGroup
            ><Kbd class="h-5 min-w-5 px-1"><ArrowLeft size={10} /></Kbd><Kbd class="h-5 min-w-5 px-1"
              ><ArrowRight size={10} /></Kbd
            ></KbdGroup
          >
          Navigate
        </span>
        <span class="flex items-center gap-1.5"><Kbd class="h-5 px-1">Enter</Kbd> Open Folder</span>
        <span class="flex items-center gap-1.5"><Kbd class="h-5 px-1">Esc</Kbd> Close</span>
      </div>
    </div>
  </div>
{/if}
