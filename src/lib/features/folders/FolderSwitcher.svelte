<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { backend } from '$lib/api/backend'
  import { IconButton } from '$lib/components/ui/button'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { getRecentFolders } from '$lib/features/folders/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { getActiveFolderPath, openFolder } from '$lib/features/workbench/state.svelte'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import FolderPathBar from './FolderPathBar.svelte'
  import { ArrowDown, ArrowLeft, ArrowRight, ArrowUp, ChevronRight, Eye, EyeOff } from '$lib/icons/lucideExports'
  import type { FolderEntry } from '$lib/types/backend'
  import { logicalKey } from '$lib/utils/keyboardEvent'
  import { basename, dirname, normalizeAbsolutePath, splitRemotePath } from '$lib/utils/paths'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'
  import { isFolderSwitcherOpen, setFolderSwitcherOpen } from './switcher.svelte'
  let surfaceRef = $state<HTMLDivElement | null>(null)
  let pathInput = $state<HTMLInputElement | null>(null)
  let editing = $state(false)
  let draft = $state('')
  let pathError = $state<string | null>(null)
  // Folder the draft last asked to list; canonical redirects must not re-trigger it.
  let typedDir: string | null = null
  let beforeEdit: { containerPath: string; selectedPath: string | null } | null = null
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

  function folderDirname(path: string): string {
    const remote = splitRemotePath(path)
    if (!remote) return dirname(path) || '/'
    return `sworm://${remote.server}${dirname(remote.path) || '/'}`
  }

  let open = $derived(isFolderSwitcherOpen())
  let leftRows = $derived(
    containerEntriesPath === containerPath ? containerEntries.filter((entry) => entry.is_dir) : []
  )
  let previewEntries = $derived(selectedEntriesPath === selectedPath ? selectedEntries : [])
  let q = $derived(filterQuery.trim().toLowerCase())
  let locationPath = $derived(selectedPath ?? containerPath)

  function isMatch(entry: FolderEntry): boolean {
    return q.length === 0 || entry.name.toLowerCase().includes(q)
  }

  /** Exact name, then prefix, then substring: what the typed name most likely means. */
  function bestMatch(entries: FolderEntry[]): FolderEntry | undefined {
    if (!q) return undefined
    const name = (entry: FolderEntry) => entry.name.toLowerCase()
    return (
      entries.find((entry) => name(entry) === q) ??
      entries.find((entry) => name(entry).startsWith(q)) ??
      entries.find(isMatch)
    )
  }

  function drillInto(path: string): void {
    containerPath = path
    selectedPath = null
    filterQuery = ''
  }

  function goUp(): void {
    const parent = folderDirname(containerPath)
    if (parent === containerPath) return
    const previousPath = containerPath
    containerPath = parent
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

  function navigateTo(path: string): void {
    if (basename(path).startsWith('.')) showHidden = true
    const parent = folderDirname(path)
    // A root has no parent row to select, so it opens as the container.
    if (parent === path) drillInto(path)
    else {
      containerPath = parent
      selectedPath = path
      filterQuery = ''
    }
    focusSelectedFolder(true)
  }

  /**
   * Read the typed location as `<folder to list>/<name being typed>`:
   * `…/rust/hypr` lists `rust` filtered by `hypr`; `…/hyprmin/` lists
   * `hyprmin`. A bare name filters the current folder (`dir: null`).
   * `null` means an incomplete or unusable location (`sworm://de`, `a/b`).
   */
  function parseDraft(value: string): { dir: string | null; leaf: string } | null {
    const remote = /^sworm:\/\/([^/]+)(\/.*)$/.exec(value)
    const path = remote ? remote[2] : value
    if (!path.startsWith('/')) {
      return path.includes('/') || value.startsWith('sworm:') || value.startsWith('~')
        ? null
        : { dir: null, leaf: value }
    }
    const slash = path.lastIndexOf('/')
    const prefix = remote ? `sworm://${remote[1]}` : ''
    return { dir: normalizeAbsolutePath(prefix + (path.slice(0, slash) || '/')), leaf: path.slice(slash + 1) }
  }

  async function startEdit(text: string, selectAll: boolean): Promise<void> {
    if (!editing) beforeEdit = { containerPath, selectedPath }
    editing = true
    draft = text
    pathError = null
    typedDir = null
    await tick()
    pathInput?.focus()
    if (selectAll) pathInput?.select()
    else pathInput?.setSelectionRange(text.length, text.length)
  }

  /** `restore` (Escape) returns to where browsing was before typing began. */
  function endEdit(restore: boolean): void {
    if (restore && beforeEdit) {
      containerPath = beforeEdit.containerPath
      selectedPath = beforeEdit.selectedPath
    }
    beforeEdit = null
    editing = false
    filterQuery = ''
    pathError = null
  }

  function applyDraft(): void {
    pathError = null
    const parsed = parseDraft(draft)
    filterQuery = parsed?.leaf ?? ''
    if (parsed?.leaf.startsWith('.')) showHidden = true
    if (parsed?.dir && parsed.dir !== typedDir) {
      typedDir = parsed.dir
      if (parsed.dir !== containerPath) {
        containerPath = parsed.dir
        selectedPath = null
      }
    }
    // A new container selects its best match once listed.
    const match = bestMatch(leftRows)
    if (match) selectedPath = match.path
  }

  /** Tab: complete the typed name to the selected folder and step inside it. */
  function completeDraft(): boolean {
    const selected = leftRows.find((entry) => entry.path === selectedPath)
    const parsed = parseDraft(draft)
    if (!q || !parsed || !selected || !isMatch(selected)) return false
    draft = parsed.dir ? `${draft.slice(0, draft.lastIndexOf('/') + 1)}${selected.name}/` : `${selected.path}/`
    applyDraft()
    return true
  }

  function submitDraft(): void {
    const parsed = parseDraft(draft)
    if (!parsed) {
      pathError = 'Type a folder name, an absolute path, or a sworm://server/path location.'
      return
    }
    if (containerEntriesPath !== containerPath) {
      pathError = containerError ?? 'Still loading this folder.'
      return
    }
    // A trailing slash names the listed folder itself.
    if (parsed.dir && !parsed.leaf) {
      void enterFolder(containerPath)
      return
    }
    const selected = leftRows.find((entry) => entry.path === selectedPath)
    if (!selected || !isMatch(selected)) {
      pathError = `No folder matches “${parsed.leaf}”.`
      return
    }
    void enterFolder(selected.path)
  }

  function handlePathKeyDown(event: KeyboardEvent): void {
    const key = logicalKey(event)
    if (key === 'Tab') {
      if (event.shiftKey || !completeDraft()) return
      event.preventDefault()
      event.stopPropagation()
      return
    }
    // Every other key stays in the entry; only these reach the list.
    event.stopPropagation()
    if (key === 'Enter') {
      event.preventDefault()
      submitDraft()
    } else if (key === 'Escape') {
      event.preventDefault()
      endEdit(true)
      focusSelectedFolder(true)
    } else if (key === 'ArrowDown' || key === 'ArrowUp') {
      event.preventDefault()
      moveSelection(key === 'ArrowDown' ? 1 : -1)
    } else if (event.ctrlKey && key.toLowerCase() === 'l') {
      event.preventDefault()
      pathInput?.select()
    }
  }

  function focusSelectedFolder(force = false): void {
    void tick().then(() => {
      if (!isFolderSwitcherOpen()) return
      if (!force && document.activeElement === pathInput) return
      const selectedIndex = leftRows.findIndex((entry) => entry.path === selectedPath)
      const index = selectedIndex >= 0 ? selectedIndex : 0
      const row = surfaceRef?.querySelector<HTMLElement>(`[data-left-index="${index}"]`)
      row?.scrollIntoView({ block: 'nearest' })
      row?.querySelector<HTMLButtonElement>('button')?.focus()
    })
  }

  /** Steps through matching rows only; focus stays in the entry while typing. */
  function moveSelection(delta: number): void {
    const rows = q ? leftRows.filter(isMatch) : leftRows
    if (rows.length === 0) return
    const currentIndex = rows.findIndex((entry) => entry.path === selectedPath)
    const next = rows[Math.max(0, Math.min(rows.length - 1, Math.max(currentIndex, 0) + delta))]
    selectedPath = next.path
    void tick().then(() => {
      const row = surfaceRef?.querySelector<HTMLElement>(`[data-left-index="${leftRows.indexOf(next)}"]`)
      row?.scrollIntoView({ block: 'nearest' })
      if (!editing) row?.querySelector<HTMLButtonElement>('button')?.focus()
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

    if (event.ctrlKey && !event.shiftKey && !event.altKey && key.toLowerCase() === 'l') {
      event.preventDefault()
      void startEdit(locationPath, true)
      return
    }

    // Type-ahead: any printable key starts typing a location in the listed
    // folder; `/` starts an absolute path.
    if (key.length === 1 && key !== ' ' && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault()
      void startEdit(key === '/' ? '/' : `${containerPath.replace(/\/$/, '')}/${key}`, false)
      applyDraft()
      return
    }

    if (
      key === 'Enter' &&
      target instanceof Element &&
      (target.closest('a[href]') || target.closest('button:not([tabindex="-1"])'))
    ) {
      return
    }

    if (key === 'ArrowDown' || key === 'ArrowUp') {
      event.preventDefault()
      moveSelection(key === 'ArrowDown' ? 1 : -1)
      return
    }

    if (key === 'ArrowRight') {
      if (!selectedPath) return
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
      event.preventDefault()
      goUp()
      return
    }
    if (key === 'Enter') {
      if (!selectedPath) return
      event.preventDefault()
      void enterFolder(selectedPath)
      return
    }
    if (key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      setFolderSwitcherOpen(false)
    }
  }

  $effect(() => {
    const opened = open
    return untrack(() => {
      if (!opened) return
      const previousFocus = document.activeElement
      const initialPath = getActiveFolderPath() ?? getRecentFolders()[0]?.path ?? null
      if (initialPath && basename(initialPath).startsWith('.')) {
        showHidden = true
      }
      containerPath = initialPath ? folderDirname(initialPath) : '/'
      selectedPath = initialPath
      endEdit(false)
      // The surface takes keys (type-ahead, arrows) before the list loads.
      void tick().then(() => {
        if (isFolderSwitcherOpen()) surfaceRef?.focus()
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
        selectedPath = (bestMatch(folders) ?? selected ?? folders[0])?.path ?? null
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
        <FolderPathBar
          path={locationPath}
          {editing}
          bind:draft
          bind:input={pathInput}
          error={pathError}
          onnavigate={navigateTo}
          onedit={() => void startEdit(locationPath, true)}
          oninput={applyDraft}
          onkeydown={handlePathKeyDown}
          onblur={() => {
            // Leaving the window keeps the draft; clicking elsewhere in the switcher ends it.
            if (document.hasFocus()) endEdit(false)
          }}
        />
        <IconButton
          size="md"
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
        <span class="flex items-center gap-1.5"><Kbd class="h-5 px-1">Ctrl+L</Kbd> Type Path</span>
        <span class="flex items-center gap-1.5"><Kbd class="h-5 px-1">Tab</Kbd> Complete</span>
        <span class="flex items-center gap-1.5"><Kbd class="h-5 px-1">Esc</Kbd> Close</span>
      </div>
    </div>
  </div>
{/if}
