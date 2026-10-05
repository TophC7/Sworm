<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { SvelteMap } from 'svelte/reactivity'
  import { backend } from '$lib/api/backend'
  import { Button, IconButton } from '$lib/components/ui/button'
  import {
    PaletteDialog,
    PaletteFooter,
    PaletteSearch,
    commandItemVariants,
    commandGroupHeadingVariants
  } from '$lib/components/ui/command'
  import { Kbd } from '$lib/components/ui/kbd'
  import { openFolderPicker } from '$lib/features/app-actions/actions.svelte'
  import { getRecentFolders } from '$lib/features/folders/state.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { remoteDotClass } from '$lib/features/remotes/remoteDot'
  import { openRemoteManager } from '$lib/features/remotes/state.svelte'
  import { getActiveFolderPath } from '$lib/features/workbench/state.svelte'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import {
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ChevronRight,
    Eye,
    EyeOff,
    FolderOpenIcon,
    Layers,
    MonitorIcon,
    ServerIcon,
    SettingsIcon
  } from '$lib/icons/lucideExports'
  import { platform } from '$lib/platform'
  import type { FolderEntry, WorkbenchInfo } from '$lib/types/backend'
  import { cn } from '$lib/utils/cn'
  import { logicalKey } from '$lib/utils/keyboardEvent'
  import { basename, dirname, normalizeAbsolutePath, splitRemotePath } from '$lib/utils/paths'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'
  import PathBar from './PathBar.svelte'
  import {
    getServers,
    getWorkbenchSections,
    watchPlaces,
    workbenchDetail,
    workbenchTitle,
    type ServerState
  } from './places.svelte'
  import { localHostLabel, closeBrowser, getBrowserRequest, goToFolder, goToWorkbench } from './state.svelte'

  type Location = { key: string; label: string; detail: string; section: string } & (
    | { kind: 'folder'; path: string }
    | { kind: 'workbench'; workbench: WorkbenchInfo; server?: string }
    | { kind: 'server'; server: string | null; state: ServerState }
    | { kind: 'manage' }
  )
  type EditOrigin = { path: string | null; selectedKey: string | null; search: string }

  const id = $props.id()
  const pathLike = /^(\/|~|sworm:)/
  const homes = new SvelteMap<string, string>()
  const homeLookups = new Map<string, Promise<string>>()
  const folderLoad = createTrackedAsyncLoad<string | null>()
  const previewLoad = createTrackedAsyncLoad<string | null>()
  let request = $derived(getBrowserRequest())
  let open = $derived(request !== null)
  let input = $state<HTMLInputElement | null>(null)
  let list = $state<HTMLDivElement | null>(null)
  // Null is the locations list. Servers are locations, never a filesystem parent.
  let path = $state<string | null>(null)
  let search = $state('')
  let selectedKey = $state<string | null>(null)
  let editOrigin = $state<EditOrigin | null>(null)
  let showHidden = $state(false)
  let entries = $state<FolderEntry[]>([])
  let entriesPath = $state<string | null>(null)
  let folderError = $state<string | null>(null)
  let pathError = $state<string | null>(null)
  let preview = $state<FolderEntry[]>([])
  let previewPath = $state<string | null>(null)
  let previewError = $state<string | null>(null)
  let enteringServer = $state<string | null>(null)
  let navigation = 0
  let pointerPosition: { x: number; y: number } | null = null

  let currentFolder = $derived(getActiveFolderPath())
  let currentServer = $derived(
    splitRemotePath((editOrigin ? editOrigin.path : path) ?? currentFolder ?? '')?.server ?? null
  )
  let editing = $derived(editOrigin !== null)
  let parsed = $derived(editing ? parseLocation(search) : null)
  let term = $derived((editing ? (parsed?.leaf ?? '') : search).trim().toLowerCase())

  function folderLocation(folderPath: string, section: string): Location {
    const remote = splitRemotePath(folderPath)
    const local = remote?.path ?? folderPath
    return {
      key: `folder:${folderPath}`,
      kind: 'folder',
      section,
      path: folderPath,
      label: basename(local) || '/',
      detail: `${remote?.server ?? localHostLabel()} · ${local}`
    }
  }

  let locations = $derived.by<Location[]>(() => [
    ...(currentFolder ? [folderLocation(currentFolder, 'Current')] : []),
    ...getRecentFolders()
      .filter(({ path: recent }) => recent !== currentFolder)
      .slice(0, term ? undefined : 8)
      .map(({ path: recent }) => folderLocation(recent, 'Recent')),
    ...getWorkbenchSections().flatMap(({ server, workbenches }) =>
      workbenches.map((workbench): Location => ({
        key: `workbench:${server ?? ''}:${workbench.id}`,
        kind: 'workbench',
        section: 'Workbenches',
        label: workbenchTitle(workbench),
        detail: workbenchDetail(workbench, server),
        workbench,
        server
      }))
    ),
    {
      key: 'server:',
      kind: 'server',
      section: 'Servers',
      label: localHostLabel(),
      detail: 'Working directory',
      server: null,
      state: 'connected'
    },
    ...getServers().map(({ name, state, lastError }): Location => ({
      key: `server:${name}`,
      kind: 'server',
      section: 'Servers',
      label: name,
      detail: lastError ?? state.charAt(0).toUpperCase() + state.slice(1),
      server: name,
      state
    })),
    ...(platform.native
      ? [{ key: 'manage', kind: 'manage', section: 'Servers', label: 'Pair or Manage Servers…', detail: '' } as const]
      : [])
  ])
  let rows = $derived.by<Location[]>(() => {
    const source =
      path === null
        ? locations
        : entriesPath === path
          ? entries.filter((entry) => entry.is_dir).map((entry) => folderLocation(entry.path, 'Folders'))
          : []
    return source.filter((row) => {
      if (editing && parsed?.servers && row.kind !== 'server') return false
      return !term || (path === null ? `${row.label} ${row.detail}` : row.label).toLowerCase().includes(term)
    })
  })
  let selected = $derived(rows.find((row) => row.key === selectedKey) ?? rows[0] ?? null)
  let selectedPath = $derived(selected?.kind === 'folder' ? selected.path : null)
  let visiblePreview = $derived(previewPath === selectedPath ? preview : [])

  function focusSearch(): void {
    void tick().then(() => input?.focus())
  }

  function scrollSelection(): void {
    void tick().then(() => list?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' }))
  }

  function showLocations(servers = false): void {
    const server = currentServer
    navigation++
    enteringServer = null
    path = null
    search = ''
    editOrigin = null
    pathError = null
    selectedKey = servers ? `server:${server ?? ''}` : null
    focusSearch()
    scrollSelection()
  }

  function browse(folderPath: string, selection: string | null = null): void {
    navigation++
    enteringServer = null
    path = folderPath
    selectedKey = selection
    search = ''
    editOrigin = null
    pathError = null
    if (basename(folderPath).startsWith('.')) showHidden = true
    focusSearch()
  }

  function parentOf(folderPath: string): string {
    const remote = splitRemotePath(folderPath)
    return remote ? `sworm://${remote.server}${dirname(remote.path) || '/'}` : dirname(folderPath) || '/'
  }

  function goUp(): void {
    if (!path) return
    const parent = parentOf(path)
    if (parent !== path) browse(parent, `folder:${path}`)
  }

  async function enterServer(server: string | null): Promise<void> {
    const generation = ++navigation
    const opening = request
    enteringServer = server ?? localHostLabel()
    pathError = null
    try {
      const workingPath = await backend.folders.workingDirectory(server ?? undefined)
      if (generation === navigation && opening === request) browse(workingPath)
    } catch (error) {
      if (generation === navigation && opening === request) pathError = getErrorMessage(error)
    } finally {
      if (generation === navigation) enteringServer = null
    }
  }

  function activate(row: Location, drill = false): void {
    switch (row.kind) {
      case 'folder':
        if (drill) browse(row.path)
        else void goToFolder(row.path, request?.replaceTabId)
        break
      case 'server':
        void enterServer(row.server)
        break
      case 'workbench':
        if (!drill) void goToWorkbench(row.workbench, row.server, false, request?.replaceTabId)
        break
      case 'manage':
        if (!drill) {
          closeBrowser()
          openRemoteManager()
        }
        break
    }
  }

  function homeOf(server: string | null): Promise<string> {
    const key = server ?? ''
    const cached = homes.get(key)
    if (cached) return Promise.resolve(cached)
    let lookup = homeLookups.get(key)
    if (!lookup) {
      lookup = backend.folders
        .home(server ?? undefined)
        .then((home) => {
          homes.set(key, home)
          return home
        })
        .finally(() => homeLookups.delete(key))
      homeLookups.set(key, lookup)
    }
    return lookup
  }

  function parseLocation(value: string): { servers: boolean; dir: string | null; leaf: string } | null {
    const hostDraft = /^sworm:\/\/([^/]*)$/.exec(value)
    if (hostDraft) return { servers: true, dir: null, leaf: hostDraft[1] }
    if (value === '~' || value.startsWith('~/')) {
      const home = homes.get(currentServer ?? '')
      if (!home) return null
      value = `${home.replace(/\/$/, '')}/${value.slice(2)}`
    }
    const remote = splitRemotePath(value)
    const local = remote?.path ?? value
    if (!local.startsWith('/')) return null
    const slash = local.lastIndexOf('/')
    const server = remote?.server ?? currentServer
    const prefix = server ? `sworm://${server}` : ''
    return {
      servers: false,
      dir: normalizeAbsolutePath(prefix + (local.slice(0, slash) || '/')),
      leaf: local.slice(slash + 1)
    }
  }

  async function applyLocation(): Promise<void> {
    const generation = ++navigation
    enteringServer = null
    pathError = null
    if (search === '~' || search.startsWith('~/')) {
      try {
        await homeOf(currentServer)
      } catch (error) {
        if (generation === navigation) pathError = getErrorMessage(error)
        return
      }
    }
    if (generation !== navigation || !editing || !open) return
    const location = parseLocation(search)
    if (!location) return
    if (location.leaf.startsWith('.')) showHidden = true
    path = location.servers ? null : location.dir
    selectedKey = null
  }

  function handleInput(): void {
    navigation++
    enteringServer = null
    pathError = null
    selectedKey = null
    if (!editing && pathLike.test(search)) editOrigin = { path, selectedKey: null, search: '' }
    if (editing) void applyLocation()
  }

  async function editLocation(): Promise<void> {
    if (!editing) editOrigin = { path, selectedKey, search }
    search = path ?? currentFolder ?? ''
    if (!search) {
      const generation = ++navigation
      const opening = request
      try {
        const workingPath = await backend.folders.workingDirectory()
        if (generation !== navigation || opening !== request || !editing) return
        search = workingPath
      } catch (error) {
        if (generation === navigation && opening === request) pathError = getErrorMessage(error)
      }
    }
    if (search) {
      search = `${search.replace(/\/$/, '')}/`
      await applyLocation()
    }
    await tick()
    input?.focus()
    input?.select()
  }

  function cancelEdit(): void {
    if (!editOrigin) return
    navigation++
    path = editOrigin.path
    selectedKey = editOrigin.selectedKey
    search = editOrigin.search
    editOrigin = null
    pathError = null
    focusSearch()
  }

  function submitLocation(): void {
    if (!parsed) {
      pathError = 'Enter an absolute path, ~, or sworm://server/path.'
    } else if (!parsed.servers && !parsed.leaf && path) {
      if (entriesPath === path) void goToFolder(path, request?.replaceTabId)
      else pathError = folderError ?? 'Still loading this folder.'
    } else if (selected) {
      activate(selected)
    } else {
      pathError = 'No matching location.'
    }
  }

  function moveSelection(delta: number): void {
    const index = rows.findIndex((row) => row.key === selected?.key)
    selectedKey = rows[Math.max(0, Math.min(rows.length - 1, index + delta))]?.key ?? null
    scrollSelection()
  }

  function handleSearchKey(event: KeyboardEvent): void {
    const key = logicalKey(event)
    if (key === 'ArrowDown' || key === 'ArrowUp') {
      event.preventDefault()
      event.stopPropagation()
      moveSelection(key === 'ArrowDown' ? 1 : -1)
    } else if (key === 'Enter') {
      event.preventDefault()
      event.stopPropagation()
      if (event.ctrlKey && path) void goToFolder(path, request?.replaceTabId)
      else if (editing) submitLocation()
      else if (selected) activate(selected)
    } else if (key === 'Escape' && editing) {
      event.preventDefault()
      event.stopPropagation()
      cancelEdit()
    } else if (key === 'Tab' && editing && !event.shiftKey && selected?.kind === 'folder') {
      event.preventDefault()
      search = `${selected.path.replace(/\/$/, '')}/`
      void applyLocation()
    } else if (!editing && (search === '' || event.altKey) && (key === 'ArrowRight' || key === 'ArrowLeft')) {
      event.preventDefault()
      event.stopPropagation()
      if (key === 'ArrowLeft') goUp()
      else if (selected) activate(selected, true)
    }
  }

  function handleKey(event: KeyboardEvent): void {
    if (event.ctrlKey && !event.altKey && logicalKey(event).toLowerCase() === 'l') {
      event.preventDefault()
      event.stopPropagation()
      void editLocation()
    } else if (event.altKey && logicalKey(event) === 'Home') {
      event.preventDefault()
      event.stopPropagation()
      showLocations()
    }
  }

  function pointerSelect(event: PointerEvent, row: Location): void {
    // Scrolling under a stationary pointer must not undo keyboard selection.
    const moved = pointerPosition
      ? pointerPosition.x !== event.clientX || pointerPosition.y !== event.clientY
      : event.movementX !== 0 || event.movementY !== 0
    pointerPosition = { x: event.clientX, y: event.clientY }
    if (moved) selectedKey = row.key
  }


  $effect(() => {
    const opening = request
    untrack(() => {
      navigation++
      if (!opening) return
      showLocations()
      if (opening.server !== undefined) void enterServer(opening.server)
      else if (opening.path) browse(opening.path)
    })
  })

  $effect(() => {
    if (open) return watchPlaces()
  })

  $effect(() => {
    const target = path
    const hidden = showHidden
    const key = open && target ? `${target}\u0000${hidden}` : null
    folderLoad.run(key, async (isCurrent) => {
      entries = []
      entriesPath = null
      folderError = null
      if (key === null || target === null) return
      try {
        const resolved = await backend.folders.resolve(target)
        if (!isCurrent()) return
        if (resolved.path !== target) {
          path = resolved.path
          return
        }
        const result = await backend.folders.listEntries(target, hidden)
        if (!isCurrent()) return
        entries = result
        entriesPath = target
        scrollSelection()
      } catch (error) {
        if (isCurrent()) folderError = getErrorMessage(error)
      }
    })
  })

  $effect(() => {
    const target = selectedPath
    const hidden = showHidden
    const key = open && path && target ? `${target}\u0000${hidden}` : null
    previewLoad.run(key, async (isCurrent) => {
      preview = []
      previewPath = null
      previewError = null
      if (key === null || target === null) return
      try {
        const result = await backend.folders.listEntries(target, hidden)
        if (!isCurrent()) return
        preview = result
        previewPath = target
      } catch (error) {
        if (isCurrent()) previewError = getErrorMessage(error)
      }
    })
  })
</script>

{#snippet locationIcon(row: Location)}
  {#if row.kind === 'folder'}
    <FileIcon filename={row.label} folder expanded={selected?.key === row.key} size={16} />
  {:else if row.kind === 'workbench'}<Layers />
  {:else if row.kind === 'server'}
    {#if row.server}<ServerIcon />{:else}<MonitorIcon />{/if}
  {:else}<SettingsIcon />{/if}
{/snippet}

<PaletteDialog
  {open}
  onOpenChange={(value) => {
    if (!value) closeBrowser()
  }}
  label="Open Folder"
  onkeydown={handleKey}
  onOpenAutoFocus={(event) => {
    event.preventDefault()
    focusSearch()
  }}
>
  <PaletteSearch>
    {#snippet children(inputClass)}
      <input
        bind:this={input}
        bind:value={search}
        class={cn(inputClass, editing && 'font-mono')}
        role="combobox"
        aria-label={editing ? 'Location' : path ? 'Filter folders or enter a path' : 'Search locations or enter a path'}
        aria-expanded={true}
        aria-controls={`${id}-locations`}
        aria-activedescendant={selected ? `${id}-row-${rows.indexOf(selected)}` : undefined}
        aria-autocomplete="list"
        aria-invalid={pathError !== null}
        placeholder={editing
          ? 'Enter a path…'
          : path
            ? 'Filter folders or enter a path…'
            : 'Search locations or enter a path…'}
        spellcheck={false}
        autocomplete="off"
        oninput={handleInput}
        onkeydown={handleSearchKey}
      />
    {/snippet}
  </PaletteSearch>

  <div class="flex min-w-0 items-center gap-1 border-b border-edge bg-surface/60 px-2 py-1">
    {#if path}
      <IconButton size="md" tooltip="Locations" shortcut="Alt+Home" onclick={() => showLocations()}>
        <ArrowLeft size={14} />
      </IconButton>
      <PathBar {path} onnavigate={browse} onhosts={() => showLocations(true)} />
    {:else}
      <span class="min-w-0 flex-1 px-1 text-xs text-muted">Locations</span>
    {/if}
    <Button variant="ghost" size="xs" onclick={() => showLocations(true)}><ServerIcon size={12} />Servers</Button>
    {#if path}
      <IconButton size="md" tooltip="Parent Folder" shortcut="Left" disabled={parentOf(path) === path} onclick={goUp}>
        <ArrowUp size={14} />
      </IconButton>
      <IconButton size="md" tooltip="Edit Location" shortcut="Ctrl+L" onclick={() => void editLocation()}>
        <FolderOpenIcon size={14} />
      </IconButton>
      <IconButton
        size="md"
        tooltip={showHidden ? 'Hide Hidden Folders' : 'Show Hidden Folders'}
        active={showHidden}
        onclick={() => {
          showHidden = !showHidden
        }}
      >
        {#if showHidden}<EyeOff size={14} />{:else}<Eye size={14} />{/if}
      </IconButton>
      <Button
        variant="ghost"
        size="xs"
        onclick={() => path && void goToFolder(path, request?.replaceTabId)}
        title="Open this folder (Ctrl+Enter)">Open Folder</Button
      >
    {/if}
    {#if platform.native}
      <IconButton
        size="md"
        tooltip="Open with System Dialog"
        onclick={() => {
          const tabId = request?.replaceTabId
          closeBrowser()
          void openFolderPicker(tabId)
        }}
      >
        <FolderOpenIcon size={14} />
      </IconButton>
    {/if}
  </div>

  {#if pathError || enteringServer}
    <div class="px-3 py-2 text-sm {pathError ? 'text-danger' : 'text-muted'}" role="status">
      {pathError ?? `Opening ${enteringServer}…`}
    </div>
  {/if}
  <div class={cn('grid min-h-0', path && 'grid-cols-2')}>
    <div
      bind:this={list}
      id={`${id}-locations`}
      role="listbox"
      aria-label={path ? 'Folders' : 'Locations'}
      aria-busy={folderLoad.loading || enteringServer !== null}
      class="max-h-[50vh] min-h-0 [scroll-padding-block:0.5rem] overflow-y-auto p-1"
    >
      {#each rows as row, index (row.key)}
        {#if index === 0 || rows[index - 1].section !== row.section}
          {#if index > 0}<div class="-mx-1 my-1 h-px bg-edge"></div>{/if}
          <div class={commandGroupHeadingVariants()}>{row.section}</div>
        {/if}
        <button
          type="button"
          role="option"
          id={`${id}-row-${index}`}
          tabindex="-1"
          aria-selected={selected?.key === row.key}
          data-selected={selected?.key === row.key ? '' : undefined}
          class={cn(commandItemVariants(), 'w-full min-w-0 text-left focus-visible:shadow-focus-ring')}
          onpointermove={(event) => pointerSelect(event, row)}
          onmousedown={(event) => event.preventDefault()}
          onclick={() => activate(row, row.kind === 'folder')}
        >
          {@render locationIcon(row)}
          <span class="min-w-0 truncate">{row.label}</span>
          {#if row.kind === 'server' && row.server}
            <span class="size-2 shrink-0 rounded-full {remoteDotClass(row.state)}" aria-label={row.state}></span>
          {/if}
          {#if !path && row.detail}<span class="ml-auto min-w-0 truncate text-xs text-subtle" title={row.detail}
              >{row.detail}</span
            >{/if}
          {#if row.kind === 'folder' || row.kind === 'server'}<ChevronRight class={path ? 'ml-auto' : ''} />{/if}
        </button>
      {:else}
        <div class="px-2 py-6 text-center text-sm text-muted" role="status">
          {#if folderError}<span class="text-danger">{folderError}</span>
          {:else if folderLoad.loading}Loading…
          {:else if term}No matching {path ? 'folders' : 'locations'}.
          {:else}No folders.{/if}
        </div>
      {/each}
    </div>
    {#if path}
      <section
        class="max-h-[50vh] min-h-0 overflow-y-auto border-l border-edge bg-surface p-1"
        aria-label="Folder contents"
      >
        <div class={cn(commandGroupHeadingVariants(), 'truncate font-mono')}>
          {selected?.kind === 'folder' ? `${selected.label}/` : 'Folder contents'}
        </div>
        {#if previewError}<p class="px-2 py-2 text-sm text-danger">{previewError}</p>
        {:else if previewLoad.loading}<p class="px-2 py-2 text-sm text-muted">Loading…</p>
        {:else if visiblePreview.length === 0}<p class="px-2 py-2 text-sm text-subtle">
            {selectedPath ? 'Empty folder.' : 'Select a folder.'}
          </p>
        {:else}
          {#each visiblePreview as entry (entry.path)}
            {#if entry.is_dir}
              <button
                type="button"
                tabindex="-1"
                class={cn(
                  commandItemVariants(),
                  'w-full min-w-0 text-left hover:bg-overlay focus-visible:shadow-focus-ring'
                )}
                onclick={() => browse(entry.path)}
              >
                <FileIcon filename={entry.name} folder size={16} /><span class="truncate">{entry.name}</span
                ><ChevronRight class="ml-auto" />
              </button>
            {:else}
              <div class="flex min-w-0 items-center gap-2 px-2 py-1.5 text-sm text-muted">
                <FileIcon filename={entry.name} size={16} /><span class="truncate">{entry.name}</span>
              </div>
            {/if}
          {/each}
        {/if}
      </section>
    {/if}
  </div>
  <PaletteFooter>
    <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
      <span class="flex items-center gap-1.5"
        ><Kbd><ArrowUp size={12} /></Kbd><Kbd><ArrowDown size={12} /></Kbd>navigate</span
      >
      <span class="flex items-center gap-1.5"><Kbd>Enter</Kbd>{selected?.kind === 'server' ? 'browse' : 'open'}</span>
      {#if editing}
        <span class="flex items-center gap-1.5"><Kbd>Tab</Kbd>complete</span>
      {:else}
        {#if selected?.kind === 'folder'}
          <span class="flex items-center gap-1.5"
            ><Kbd
              >{#if search}Alt+{/if}<ArrowRight size={12} /></Kbd
            >browse</span
          >
        {/if}
        {#if path}<span class="flex items-center gap-1.5"
            ><Kbd
              >{#if search}Alt+{/if}<ArrowLeft size={12} /></Kbd
            >parent</span
          >{/if}
        <span class="flex items-center gap-1.5"><Kbd>Ctrl+L</Kbd>path</span>
      {/if}
    </div>
    <span class="flex shrink-0 items-center gap-1.5"><Kbd>Esc</Kbd>{editing ? 'cancel' : 'close'}</span>
  </PaletteFooter>
</PaletteDialog>
