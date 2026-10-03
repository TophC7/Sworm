<!--
  @component
  Browser — the one place to go somewhere else. Places lists workbenches,
  recent folders, and hosts; Folders browses any host in two columns, with
  the hosts list one level above each host's `/`. Typing a path in Places
  (`/`, `~`, `sworm://`) switches to Folders in place, like an address bar.
-->

<script lang="ts">
  import { tick, untrack } from 'svelte'
  import { SvelteMap } from 'svelte/reactivity'
  import { backend } from '$lib/api/backend'
  import { IconButton } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { TabsList, TabsRoot, TabsTrigger } from '$lib/components/ui/tabs'
  import { getRecentFolders } from '$lib/features/folders/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { notify } from '$lib/features/notifications/state.svelte'
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
    FolderOpen,
    Layers,
    MonitorIcon,
    ServerIcon,
    SettingsIcon
  } from '$lib/icons/lucideExports'
  import { platform, requireNative } from '$lib/platform'
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
  import {
    localHostLabel,
    closeBrowser,
    getBrowserRequest,
    goToFolder,
    goToWorkbench,
    isBrowserOpen
  } from './state.svelte'

  type Mode = 'places' | 'folders'

  /** A host row; `server` null is this host. */
  interface Host {
    server: string | null
    label: string
    state: ServerState
    lastError: string | null
  }

  type Place = { key: string; label: string; detail: string } & (
    | { kind: 'workbench'; section: 'Workbenches'; workbench: WorkbenchInfo; server?: string }
    | { kind: 'recent'; section: 'Recent'; path: string; server: string | null }
    | { kind: 'host'; section: 'Servers'; host: Host }
    | { kind: 'manage'; section: 'Servers' }
  )

  const RECENT_LIMIT = 8
  // Only desktop pairs servers, so only there does a level exist above `/`.
  const hostsLevel = platform.capabilities.remoteHosts
  const pathLike = /^(\/|~|sworm:)/

  let request = $derived(getBrowserRequest())
  let open = $derived(request !== null)
  let mode = $state<Mode>('places')

  let surfaceRef = $state<HTMLDivElement | null>(null)
  let queryInput = $state<HTMLInputElement | null>(null)
  let query = $state('')
  let placeIndex = $state(0)

  let pathInput = $state<HTMLInputElement | null>(null)
  let editing = $state(false)
  let draft = $state('')
  let pathError = $state<string | null>(null)
  // Folder the draft last asked to list; canonical redirects must not re-trigger it.
  let typedDir: string | null = null
  let beforeEdit: { atHosts: boolean; containerPath: string; selectedPath: string | null } | null = null
  let atHosts = $state(false)
  let selectedHost = $state<string | null>(null)
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
  /** Home folder per host, keyed by server name (`''` for this host). */
  const homes = new SvelteMap<string, string>()
  const homeLookups = new Map<string, Promise<string | null>>()

  const containerLoad = createTrackedAsyncLoad<string | null>()
  const selectedLoad = createTrackedAsyncLoad<string | null>()

  function hostOf(path: string): string | null {
    return splitRemotePath(path)?.server ?? null
  }

  function folderDirname(path: string): string {
    const remote = splitRemotePath(path)
    if (!remote) return dirname(path) || '/'
    return `sworm://${remote.server}${dirname(remote.path) || '/'}`
  }

  /** Column heading: the folder's name, or `/` at a host's root. */
  function columnLabel(path: string): string {
    const local = splitRemotePath(path)?.path ?? path
    return local === '/' ? '/' : `${basename(local)}/`
  }

  let hosts = $derived<Host[]>([
    { server: null, label: localHostLabel(), state: 'connected', lastError: null },
    ...getServers().map(({ name, state, lastError }) => ({ server: name, label: name, state, lastError }))
  ])
  let newTabId = $derived(request?.newTabId)
  let currentHost = $derived(atHosts ? selectedHost : hostOf(containerPath))
  let q = $derived(filterQuery.trim().toLowerCase())
  let leftRows = $derived(
    containerEntriesPath === containerPath ? containerEntries.filter((entry) => entry.is_dir) : []
  )
  let previewEntries = $derived(selectedEntriesPath === selectedPath ? selectedEntries : [])
  let locationPath = $derived(atHosts ? 'sworm://' : (selectedPath ?? containerPath))
  let hostRows = $derived(q ? hosts.filter((host) => host.label.toLowerCase().includes(q)) : hosts)
  let selectedHostRow = $derived(hosts.find((host) => host.server === selectedHost) ?? null)

  // PLACES //

  function recentPlace(path: string): Place {
    const remote = splitRemotePath(path)
    const local = remote?.path ?? path
    return {
      key: `recent:${path}`,
      kind: 'recent',
      section: 'Recent',
      label: basename(local) || local,
      detail: dirname(local) || '/',
      path,
      server: remote?.server ?? null
    }
  }

  function hostDetail(host: Host): string {
    if (host.server === null) return ''
    return host.state === 'checking' ? 'Checking' : host.state.charAt(0).toUpperCase() + host.state.slice(1)
  }

  let places = $derived.by<Place[]>(() => {
    const term = query.trim().toLowerCase()
    const all: Place[] = [
      ...getWorkbenchSections().flatMap(({ server, workbenches }) =>
        workbenches.map((workbench): Place => ({
          key: `workbench:${server ?? ''}:${workbench.id}`,
          kind: 'workbench',
          section: 'Workbenches',
          label: workbenchTitle(workbench),
          detail: workbenchDetail(workbench, server),
          workbench,
          server
        }))
      ),
      ...getRecentFolders()
        .slice(0, term ? undefined : RECENT_LIMIT)
        .map(({ path }) => recentPlace(path)),
      ...hosts.map((host): Place => ({
        key: `host:${host.server ?? ''}`,
        kind: 'host',
        section: 'Servers',
        label: host.label,
        detail: hostDetail(host),
        host
      })),
      ...(hostsLevel
        ? [{ key: 'manage', kind: 'manage', section: 'Servers', label: 'Pair or Manage Servers…', detail: '' } as const]
        : [])
    ]
    if (!term) return all
    return all.filter((place) =>
      [place.label, place.detail, place.kind === 'recent' ? (place.server ?? '') : ''].some((text) =>
        text.toLowerCase().includes(term)
      )
    )
  })

  function activatePlace(place: Place): void {
    switch (place.kind) {
      case 'workbench':
        void goToWorkbench(place.workbench, place.server, false, newTabId)
        return
      case 'recent':
        void goToFolder(place.path, newTabId)
        return
      case 'host':
        void enterHost(place.host.server)
        return
      case 'manage':
        closeBrowser()
        openRemoteManager()
        return
    }
  }

  function movePlace(delta: number): void {
    if (places.length === 0) return
    placeIndex = Math.max(0, Math.min(places.length - 1, placeIndex + delta))
    void tick().then(() =>
      surfaceRef?.querySelector(`[data-place-index="${placeIndex}"]`)?.scrollIntoView({ block: 'nearest' })
    )
  }

  function handleQueryInput(): void {
    placeIndex = 0
    // A path is somewhere to browse, not something to search for.
    if (pathLike.test(query)) switchToFolders(query)
  }

  function handleQueryKeyDown(event: KeyboardEvent): void {
    const key = logicalKey(event)
    if (key === 'ArrowDown' || key === 'ArrowUp') {
      event.preventDefault()
      event.stopPropagation()
      movePlace(key === 'ArrowDown' ? 1 : -1)
    } else if (key === 'Enter') {
      event.preventDefault()
      event.stopPropagation()
      const place = places[placeIndex]
      if (place) activatePlace(place)
    } else if (key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      closeBrowser()
    }
  }

  function switchToPlaces(): void {
    endEdit(false)
    mode = 'places'
    query = ''
    placeIndex = 0
    void tick().then(() => queryInput?.focus())
  }

  /** Enter Folders at the current folder; `text` starts typing a location there. */
  function switchToFolders(text?: string): void {
    mode = 'folders'
    query = ''
    showFolder(getActiveFolderPath() ?? getRecentFolders()[0]?.path ?? homes.get('') ?? '/')
    if (text === undefined) {
      focusSelectedFolder(true)
      return
    }
    void startEdit(text, false)
    applyDraft()
  }

  // HOSTS //

  /** A host's canonical home folder; concurrent asks share one lookup, failures are retried next time. */
  function homeOf(server: string | null): Promise<string | null> {
    const key = server ?? ''
    const cached = homes.get(key)
    if (cached) return Promise.resolve(cached)
    let lookup = homeLookups.get(key)
    if (!lookup) {
      lookup = backend.folders.home(server ?? undefined).then(
        (home) => {
          homes.set(key, home)
          return home
        },
        () => null
      )
      homeLookups.set(key, lookup)
      void lookup.finally(() => homeLookups.delete(key))
    }
    return lookup
  }

  /** Browse inside a host's home; an unreachable server opens at its root so the listing says why. */
  async function enterHost(server: string | null): Promise<void> {
    const home = await homeOf(server)
    if (!isBrowserOpen()) return
    mode = 'folders'
    endEdit(false)
    drillInto(home ?? (server ? `sworm://${server}/` : '/'))
    focusSelectedFolder(true)
  }

  function showHosts(server: string | null): void {
    if (!hostsLevel) return
    atHosts = true
    selectedHost = server
    filterQuery = ''
    focusSelectedFolder(true)
  }

  // FOLDERS //

  function drillInto(path: string): void {
    atHosts = false
    containerPath = path
    selectedPath = null
    filterQuery = ''
  }

  /** Select `path` in its parent; a root has no parent row, so it opens as the container. */
  function showFolder(path: string): void {
    if (basename(path).startsWith('.')) showHidden = true
    const parent = folderDirname(path)
    if (parent === path) drillInto(path)
    else {
      atHosts = false
      containerPath = parent
      selectedPath = path
      filterQuery = ''
    }
  }

  function goUp(): void {
    if (atHosts) return
    const parent = folderDirname(containerPath)
    if (parent === containerPath) {
      showHosts(hostOf(containerPath))
      return
    }
    const previousPath = containerPath
    containerPath = parent
    selectedPath = previousPath
    filterQuery = ''
  }

  function enterFolder(path: string): void {
    void goToFolder(path, newTabId)
  }

  function selectPreviewFolder(path: string): void {
    if (!selectedPath) return
    containerPath = selectedPath
    selectedPath = path
    filterQuery = ''
  }

  function navigateTo(path: string): void {
    showFolder(path)
    focusSelectedFolder(true)
  }

  /** Exact name, then prefix, then substring: what the typed name most likely means. */
  function bestMatch<T>(rows: T[], name: (row: T) => string): T | undefined {
    if (!q) return undefined
    const lower = (row: T) => name(row).toLowerCase()
    return (
      rows.find((row) => lower(row) === q) ??
      rows.find((row) => lower(row).startsWith(q)) ??
      rows.find((row) => lower(row).includes(q))
    )
  }

  /** `~` names the home folder of the host being browsed; `null` until that home is known. */
  function expandHome(value: string): string | null {
    if (value !== '~' && !value.startsWith('~/')) return value
    const home = homes.get(currentHost ?? '')
    if (!home) {
      // Only a home that arrives re-reads the draft, so a failing host cannot loop.
      void homeOf(currentHost).then((found) => found && editing && applyDraft())
      return null
    }
    return home + value.slice(1)
  }

  /**
   * Read the typed location. `sworm://lo` filters the hosts list. Otherwise
   * `<folder to list>/<name being typed>`: `…/rust/hypr` lists `rust`
   * filtered by `hypr`; `…/hyprmin/` lists `hyprmin`; a bare name filters the
   * current list (`dir: null`). `null` is an incomplete or unusable location.
   */
  function parseDraft(
    value: string
  ): { hosts: true; leaf: string } | { hosts: false; dir: string | null; leaf: string } | null {
    const hostDraft = /^sworm:\/\/([^/]*)$/.exec(value)
    if (hostDraft && hostsLevel) return { hosts: true, leaf: hostDraft[1] }
    const expanded = expandHome(value)
    if (expanded === null) return null
    const remote = splitRemotePath(expanded)
    const path = remote ? remote.path : expanded
    if (!path.startsWith('/')) {
      return path.includes('/') || expanded.startsWith('sworm:') ? null : { hosts: false, dir: null, leaf: expanded }
    }
    const slash = path.lastIndexOf('/')
    const prefix = remote ? `sworm://${remote.server}` : ''
    return {
      hosts: false,
      dir: normalizeAbsolutePath(prefix + (path.slice(0, slash) || '/')),
      leaf: path.slice(slash + 1)
    }
  }

  async function startEdit(text: string, selectAll: boolean): Promise<void> {
    if (!editing) beforeEdit = { atHosts, containerPath, selectedPath }
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
      atHosts = beforeEdit.atHosts
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
    if (!parsed) return
    if (parsed.hosts || (parsed.dir === null && atHosts)) {
      atHosts = true
      typedDir = null
      const matched = bestMatch(hosts, (host) => host.label) ?? hostRows[0]
      if (matched) selectedHost = matched.server
      return
    }
    if (parsed.leaf.startsWith('.')) showHidden = true
    if (parsed.dir && (parsed.dir !== typedDir || atHosts)) {
      typedDir = parsed.dir
      atHosts = false
      if (parsed.dir !== containerPath) {
        containerPath = parsed.dir
        selectedPath = null
      }
    }
    // A new container selects its best match once listed.
    const match = bestMatch(leftRows, (entry) => entry.name)
    if (match) selectedPath = match.path
  }

  /** Tab: complete the typed name to the selected row and step inside it. */
  function completeDraft(): boolean {
    const parsed = parseDraft(draft)
    if (!parsed) return false
    if (atHosts) {
      const host = hostRows.find((row) => row.server === selectedHost)
      if (!host) return false
      void homeOf(host.server).then((home) => {
        if (!editing) return
        draft = `${home ?? (host.server ? `sworm://${host.server}` : '')}/`
        applyDraft()
      })
      return true
    }
    const selected = leftRows.find((entry) => entry.path === selectedPath)
    if (!q || !selected || !selected.name.toLowerCase().includes(q) || parsed.hosts) return false
    draft = parsed.dir ? `${draft.slice(0, draft.lastIndexOf('/') + 1)}${selected.name}/` : `${selected.path}/`
    applyDraft()
    return true
  }

  function submitDraft(): void {
    const parsed = parseDraft(draft)
    if (!parsed) {
      pathError = /^~/.test(draft)
        ? 'Loading the home folder…'
        : 'Type a folder name, an absolute path, ~, or a sworm://server/path location.'
      return
    }
    if (atHosts) {
      const host = hostRows.find((row) => row.server === selectedHost)
      if (host) void enterHost(host.server)
      else pathError = `No server matches “${parsed.leaf}”.`
      return
    }
    if (containerEntriesPath !== containerPath) {
      pathError = containerError ?? 'Still loading this folder.'
      return
    }
    // A trailing slash names the listed folder itself.
    if (!parsed.hosts && parsed.dir && !parsed.leaf) {
      enterFolder(containerPath)
      return
    }
    const selected = leftRows.find((entry) => entry.path === selectedPath)
    if (!selected || !selected.name.toLowerCase().includes(q)) {
      pathError = `No folder matches “${parsed.leaf}”.`
      return
    }
    enterFolder(selected.path)
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
      if (!isBrowserOpen() || mode !== 'folders') return
      if (!force && document.activeElement === pathInput) return
      const attribute = atHosts ? 'data-host-index' : 'data-left-index'
      const rows = atHosts ? hostRows.map((host) => host.server) : leftRows.map((entry) => entry.path)
      const selectedIndex = rows.indexOf(atHosts ? selectedHost : selectedPath)
      const row = surfaceRef?.querySelector<HTMLElement>(`[${attribute}="${Math.max(selectedIndex, 0)}"]`)
      row?.scrollIntoView({ block: 'nearest' })
      if (row) row.querySelector<HTMLButtonElement>('button')?.focus()
      else surfaceRef?.focus()
    })
  }

  /** Steps through matching rows only; focus stays in the entry while typing. */
  function moveSelection(delta: number): void {
    if (atHosts) {
      if (hostRows.length === 0) return
      const current = hostRows.findIndex((host) => host.server === selectedHost)
      selectedHost = hostRows[Math.max(0, Math.min(hostRows.length - 1, Math.max(current, 0) + delta))].server
      if (!editing) focusSelectedFolder(true)
      return
    }
    const rows = q ? leftRows.filter((entry) => entry.name.toLowerCase().includes(q)) : leftRows
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

  function trapTab(event: KeyboardEvent): void {
    if (!surfaceRef) return
    const focusable = Array.from(
      surfaceRef.querySelectorAll<HTMLElement>('button:not([disabled]), a[href], input:not([disabled]), [tabindex="0"]')
    )
    const first = focusable[0]
    const last = focusable.at(-1)
    if (!first || !last) return
    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault()
      last.focus()
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault()
      first.focus()
    }
  }

  function handleKeyDown(event: KeyboardEvent): void {
    const target = event.target
    const key = logicalKey(event)

    if (key === 'Tab') {
      trapTab(event)
      return
    }
    if (key === 'Escape') {
      event.preventDefault()
      event.stopPropagation()
      closeBrowser()
      return
    }
    if (mode === 'places') return

    if (event.ctrlKey && !event.shiftKey && !event.altKey && key.toLowerCase() === 'l') {
      event.preventDefault()
      void startEdit(locationPath, true)
      return
    }

    // Type-ahead: any printable key starts typing a location in the listed
    // folder (or host list); `/` starts an absolute path, `~` a home path.
    if (key.length === 1 && key !== ' ' && !event.ctrlKey && !event.metaKey && !event.altKey) {
      event.preventDefault()
      const text =
        key === '/' || key === '~' ? key : atHosts ? `sworm://${key}` : `${containerPath.replace(/\/$/, '')}/${key}`
      void startEdit(text, false)
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
    if (key === 'ArrowLeft') {
      event.preventDefault()
      goUp()
      return
    }
    if (key === 'ArrowRight' || key === 'Enter') {
      if (atHosts) {
        event.preventDefault()
        void enterHost(selectedHost)
        return
      }
      if (!selectedPath) return
      event.preventDefault()
      if (key === 'Enter') {
        enterFolder(selectedPath)
        return
      }
      const targetPath = selectedPath
      if (selectedEntriesPath === targetPath) {
        if (selectedEntries.some((entry) => entry.is_dir)) drillInto(targetPath)
        return
      }
      void backend.folders.listEntries(targetPath, showHidden).then(
        (entries) => {
          if (targetPath === selectedPath && entries.some((entry) => entry.is_dir)) drillInto(targetPath)
        },
        () => {}
      )
    }
  }

  async function openWithSystemDialog(): Promise<void> {
    const tabId = newTabId
    closeBrowser()
    try {
      const path = await requireNative().dialogs.selectDirectory()
      if (path) await goToFolder(path, tabId)
    } catch (error) {
      notify.error('Open folder failed', getErrorMessage(error))
    }
  }

  // Each request starts fresh: Places, inside a host's home, or at a folder.
  $effect(() => {
    const current = request
    return untrack(() => {
      if (!current) return
      const previousFocus = document.activeElement
      endEdit(false)
      query = ''
      placeIndex = 0
      atHosts = false
      if (current.server !== undefined) {
        mode = 'folders'
        void enterHost(current.server)
      } else if (current.path) {
        mode = 'folders'
        navigateTo(current.path)
      } else {
        switchToPlaces()
      }
      void homeOf(null)
      return () => {
        void tick().then(() => {
          if (
            !isBrowserOpen() &&
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
    if (open) return watchPlaces()
  })

  // Learn the browsed host's home so `~` resolves without a wait.
  $effect(() => {
    if (open && mode === 'folders') void homeOf(currentHost)
  })

  $effect(() => {
    const requestedPath = containerPath
    const requestedHidden = showHidden
    const key = open && mode === 'folders' && !atHosts ? `${requestedPath}\u0000${requestedHidden}` : null

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
        selectedPath = (bestMatch(folders, (entry) => entry.name) ?? selected ?? folders[0])?.path ?? null
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
    const key =
      open && mode === 'folders' && !atHosts && requestedPath ? `${requestedPath}\u0000${requestedHidden}` : null

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
      if (target instanceof Element && target.closest('[data-browser-toggle="true"]')) return
      closeBrowser()
    }

    document.addEventListener('pointerdown', handlePointerDown)
    return () => document.removeEventListener('pointerdown', handlePointerDown)
  })

  const rowButtonClass =
    'relative flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1 border-none bg-transparent py-0.5 pl-2.5 text-left text-inherit focus-visible:shadow-focus-ring focus-visible:outline-none'
</script>

{#snippet dot(state: ServerState)}
  <span class="size-1.5 shrink-0 rounded-full {remoteDotClass(state)}"></span>
{/snippet}

{#snippet hostIcon(server: string | null, size: number)}
  {#if server}<ServerIcon {size} class="shrink-0" />{:else}<MonitorIcon {size} class="shrink-0" />{/if}
{/snippet}

{#snippet hint(label: string, keys: string[])}
  <span class="flex items-center gap-1.5">
    <KbdGroup>
      {#each keys as key (key)}
        <Kbd class="h-5 min-w-5 px-1">
          {#if key === 'up'}<ArrowUp size={10} />{:else if key === 'down'}<ArrowDown
              size={10}
            />{:else if key === 'left'}<ArrowLeft size={10} />{:else if key === 'right'}<ArrowRight
              size={10}
            />{:else}{key}{/if}
        </Kbd>
      {/each}
    </KbdGroup>
    {label}
  </span>
{/snippet}

{#snippet entryRow(entry: FolderEntry, pane: 'left' | 'right', index = -1)}
  {@const selected = pane === 'left' && selectedPath === entry.path}
  {@const dimmed = pane === 'left' && q.length > 0 && !entry.name.toLowerCase().includes(q)}
  <div
    style:height="22px"
    class="group/tree-row relative flex w-full items-center text-sm transition-opacity {selected
      ? 'bg-accent/10 text-bright'
      : entry.is_dir
        ? 'text-muted hover:bg-surface'
        : 'text-fg'} {dimmed ? 'opacity-30' : ''}"
    role="presentation"
    data-path={entry.path}
    data-left-index={pane === 'left' ? index : undefined}
  >
    {#if pane === 'left'}
      <button
        type="button"
        tabindex="-1"
        class={rowButtonClass}
        aria-pressed={selected}
        onclick={() => (selectedPath = entry.path)}
        onfocus={() => (selectedPath = entry.path)}
        ondblclick={() => enterFolder(entry.path)}
      >
        <FileIcon filename={entry.name} folder expanded={selected} size={15} />
        <span class="truncate">{entry.name}</span>
      </button>
      <ChevronRight size={12} class="mr-2 shrink-0 text-subtle" />
    {:else if entry.is_dir}
      <button type="button" tabindex="-1" class={rowButtonClass} onclick={() => selectPreviewFolder(entry.path)}>
        <FileIcon filename={entry.name} folder expanded={false} size={15} />
        <span class="truncate">{entry.name}</span>
      </button>
      <ChevronRight size={12} class="mr-2 shrink-0 text-subtle" />
    {:else}
      <div class="relative flex h-full min-w-0 flex-1 items-center gap-1 py-0.5 pl-2.5">
        <FileIcon filename={entry.name} size={15} />
        <span class="truncate">{entry.name}</span>
      </div>
    {/if}
  </div>
{/snippet}

{#snippet hostRow(host: Host, index: number)}
  {@const selected = selectedHost === host.server}
  <div
    class="flex h-7 w-full items-center text-sm {selected ? 'bg-accent/10 text-bright' : 'text-muted hover:bg-surface'}"
    role="presentation"
    data-host-index={index}
  >
    <button
      type="button"
      tabindex="-1"
      class={cn(rowButtonClass, 'gap-2')}
      aria-pressed={selected}
      onclick={() => (selectedHost = host.server)}
      onfocus={() => (selectedHost = host.server)}
      ondblclick={() => void enterHost(host.server)}
    >
      {@render hostIcon(host.server, 14)}
      <span class="truncate">{host.label}</span>
      {#if host.server}{@render dot(host.state)}{/if}
    </button>
    <ChevronRight size={12} class="mr-2 shrink-0 text-subtle" />
  </div>
{/snippet}

{#snippet sideRow(label: string, detail: string, onclick: () => void, icon: 'workbench' | 'folder')}
  <button
    type="button"
    tabindex="-1"
    class="flex h-7 w-full min-w-0 cursor-pointer items-center gap-2 border-none bg-transparent px-3 text-left text-sm text-fg hover:bg-surface focus-visible:shadow-focus-ring focus-visible:outline-none"
    {onclick}
  >
    {#if icon === 'workbench'}<Layers size={14} class="shrink-0 text-muted" />{:else}<FileIcon
        filename={label}
        folder
        size={15}
      />{/if}
    <span class="truncate">{label}</span>
    {#if detail}<span class="ml-auto shrink-0 truncate pl-2 font-mono text-xs text-subtle">{detail}</span>{/if}
  </button>
{/snippet}

{#snippet hostDetailPane(host: Host)}
  {@const workbenches = getWorkbenchSections().find((section) => section.server === host.server)?.workbenches ?? []}
  {@const recents = getRecentFolders()
    .filter(({ path }) => hostOf(path) === host.server)
    .slice(0, RECENT_LIMIT)}
  <div class="flex flex-col gap-1 px-3 py-2">
    <div class="flex items-center gap-2 text-base text-bright">
      {@render hostIcon(host.server, 14)}
      {host.label}
      {#if host.server}
        {@render dot(host.state)}
        <span class="text-xs text-muted">{hostDetail(host)}</span>
      {/if}
    </div>
    {#if host.lastError}<p class="m-0 text-sm text-danger">{host.lastError}</p>{/if}
    <p class="m-0 text-xs text-subtle">Enter opens the home folder.</p>
  </div>
  {#if workbenches.length > 0}
    <div class="section-label">Workbenches</div>
    {#each workbenches as workbench (workbench.id)}
      {@render sideRow(
        workbenchTitle(workbench),
        workbenchDetail(workbench),
        () => void goToWorkbench(workbench, host.server ?? undefined, false, newTabId),
        'workbench'
      )}
    {/each}
  {/if}
  {#if recents.length > 0}
    <div class="section-label">Recent</div>
    {#each recents as { path } (path)}
      {@const place = recentPlace(path)}
      {@render sideRow(place.label, place.detail, () => enterFolder(path), 'folder')}
    {/each}
  {/if}
{/snippet}

{#snippet placeRow(place: Place, index: number)}
  {@const selected = index === placeIndex}
  <button
    type="button"
    tabindex="-1"
    data-place-index={index}
    class="flex h-7 w-full min-w-0 cursor-pointer items-center gap-2 border-none px-3 text-left text-sm focus-visible:shadow-focus-ring focus-visible:outline-none {selected
      ? 'bg-accent/10 text-bright'
      : 'bg-transparent text-fg'}"
    onpointermove={() => (placeIndex = index)}
    onclick={() => activatePlace(place)}
  >
    {#if place.kind === 'workbench'}
      <Layers size={14} class="shrink-0 text-muted" />
    {:else if place.kind === 'recent'}
      <FileIcon filename={place.label} folder size={15} />
    {:else if place.kind === 'host'}
      <span class="text-muted">{@render hostIcon(place.host.server, 14)}</span>
    {:else}
      <SettingsIcon size={14} class="shrink-0 text-muted" />
    {/if}
    <span class="shrink-0 truncate">{place.label}</span>
    {#if place.kind === 'host' && place.host.server}{@render dot(place.host.state)}{/if}
    {#if place.kind === 'recent' && place.server}
      <span class="flex shrink-0 items-center gap-1 rounded-sm bg-raised px-1 text-xs text-muted">
        <ServerIcon size={10} />{place.server}
      </span>
    {/if}
    {#if place.detail}
      <span class="ml-auto min-w-0 truncate pl-3 font-mono text-xs text-subtle" title={place.detail}
        >{place.detail}</span
      >
    {/if}
  </button>
{/snippet}

{#if open}
  <div class="pointer-events-none fixed inset-x-0 top-[10vh] z-40 flex justify-center px-2">
    <div
      bind:this={surfaceRef}
      class="pointer-events-auto flex max-h-[80vh] w-[720px] max-w-full flex-col overflow-hidden rounded-xl border border-edge bg-raised shadow-popover"
      role="dialog"
      aria-modal="true"
      aria-label="Browse"
      tabindex="-1"
      onkeydown={handleKeyDown}
    >
      <div class="flex shrink-0 items-start gap-2 border-b border-edge px-3 py-2">
        <TabsRoot
          value={mode}
          onValueChange={(value) => (value === 'places' ? switchToPlaces() : switchToFolders())}
          class="shrink-0"
        >
          <TabsList>
            <TabsTrigger value="places">Places</TabsTrigger>
            <TabsTrigger value="folders">Folders</TabsTrigger>
          </TabsList>
        </TabsRoot>
        {#if mode === 'places'}
          <Input
            bind:ref={queryInput}
            bind:value={query}
            aria-label="Search places"
            placeholder="Search places, or type a path…"
            spellcheck={false}
            autocomplete="off"
            class="h-8 min-w-0 flex-1 rounded-lg py-1 text-sm"
            oninput={handleQueryInput}
            onkeydown={handleQueryKeyDown}
          />
        {:else}
          <PathBar
            path={atHosts ? null : locationPath}
            hosts={hostsLevel}
            {editing}
            bind:draft
            bind:input={pathInput}
            error={pathError}
            onnavigate={navigateTo}
            onhosts={() => showHosts(currentHost)}
            onedit={() => void startEdit(locationPath, true)}
            oninput={applyDraft}
            onkeydown={handlePathKeyDown}
            onblur={() => {
              // Leaving the window keeps the draft; clicking elsewhere in the browser ends it.
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
        {/if}
        {#if platform.capabilities.nativeDirectoryPicker}
          <IconButton size="md" tooltip="Open with System Dialog" onclick={() => void openWithSystemDialog()}>
            <FolderOpen size={12} />
          </IconButton>
        {/if}
      </div>

      {#if mode === 'places'}
        <div class="h-72 min-h-0 shrink overflow-y-auto bg-surface py-1" role="listbox" aria-label="Places">
          {#each places as place, index (place.key)}
            {#if index === 0 || places[index - 1].section !== place.section}
              <div class="section-label">{place.section}</div>
            {/if}
            {@render placeRow(place, index)}
          {:else}
            <div class="px-3 py-2 text-sm text-subtle">No places match “{query}”.</div>
          {/each}
        </div>
      {:else}
        <div class="grid h-72 min-h-0 shrink grid-cols-2">
          <section
            class="flex min-h-0 flex-col border-r border-edge bg-surface"
            aria-label={atHosts ? 'Servers' : 'Folders'}
          >
            <div class="section-label shrink-0 font-mono normal-case">
              {atHosts ? 'Servers' : columnLabel(containerPath)}
            </div>
            <div class="min-h-0 flex-1 overflow-y-auto py-1" aria-live="polite">
              {#if atHosts}
                {#each hostRows as host, index (host.server ?? '')}
                  {@render hostRow(host, index)}
                {:else}
                  <div class="px-3 py-2 text-sm text-subtle">No servers match.</div>
                {/each}
              {:else if containerLoad.loading && leftRows.length === 0}
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

          <section class="flex min-h-0 flex-col bg-ground" aria-label={atHosts ? 'Server' : 'Folder contents'}>
            {#if atHosts}
              <div class="min-h-0 flex-1 overflow-y-auto pb-1">
                {#if selectedHostRow}{@render hostDetailPane(selectedHostRow)}{/if}
              </div>
            {:else}
              <div class="section-label shrink-0 font-mono normal-case">
                {selectedPath ? columnLabel(selectedPath) : 'Folder contents'}
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
            {/if}
          </section>
        </div>
      {/if}

      <div class="flex shrink-0 items-center gap-4 border-t border-edge bg-surface px-3 py-1.5 text-xs text-subtle">
        {@render hint('Select', ['up', 'down'])}
        {#if mode === 'places'}
          {@render hint('Open', ['Enter'])}
          {@render hint('Type Path', ['/'])}
          {@render hint('Home', ['~'])}
        {:else}
          {@render hint(hostsLevel ? 'Navigate, ← at / for Servers' : 'Navigate', ['left', 'right'])}
          {@render hint(atHosts ? 'Browse' : 'Open Folder', ['Enter'])}
          {@render hint('Type Path', ['Ctrl+L'])}
          {@render hint('Complete', ['Tab'])}
        {/if}
        {@render hint('Close', ['Esc'])}
      </div>
    </div>
  </div>
{/if}
