import { backend } from '$lib/api/backend'
import type { ExplorerDirEntry } from '$lib/types/backend'
import type { FileTreeNode } from '$lib/utils/fileTree'
import { isEqualOrParent } from '$lib/utils/paths'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { SvelteMap, SvelteSet } from 'svelte/reactivity'
import { markProjectFilesStale } from '$lib/features/files/projectFiles.svelte'

// Lazy explorer tree state, one record per open folder.
//
// The backend resolves a single directory per call, so opening a folder reads
// only its root and each expand reads exactly one more directory — no listing
// is ever capped. Freshness comes from a backend watcher over exactly the
// directories rendered here, plus a refresh on window focus for the events
// Linux drops.

export type ExplorerNode = FileTreeNode<{ path: string }>

export type FolderTree = {
  /** Directory path ('' = project root) -> its listing. */
  children: SvelteMap<string, ExplorerDirEntry[]>
  expanded: SvelteSet<string>
  showHidden: boolean
  error: string | null
}

/** Coalesces the watch-set update that follows a burst of listings. */
const WATCH_SYNC_DELAY_MS = 100

const folders = new SvelteMap<string, FolderTree>()
const inflight = new Map<string, Promise<void>>()
const watchTimers = new Map<string, ReturnType<typeof setTimeout>>()
/** Last directory set handed to the backend watcher, per folder. */
const watchedSignatures = new Map<string, string>()

/**
 * Read-only view of a folder's tree state. Returns a shared empty record for
 * folders nothing has loaded yet, because this runs inside `$derived` and
 * creating state there would be a mutation during derivation. Reads still
 * register on the map key, so the first `loadDir` re-runs the derivation.
 */
export function getFolderTree(folderPath: string): FolderTree {
  return folders.get(folderPath) ?? EMPTY_TREE
}

/** Never mutated; writers go through `folderTreeFor`. */
const EMPTY_TREE: FolderTree = {
  children: new SvelteMap(),
  expanded: new SvelteSet(),
  showHidden: false,
  error: null
}

function folderTreeFor(folderPath: string): FolderTree {
  const existing = folders.get(folderPath)
  if (existing) return existing

  const tree: FolderTree = $state({
    children: new SvelteMap<string, ExplorerDirEntry[]>(),
    expanded: new SvelteSet<string>(),
    showHidden: false,
    error: null
  })
  folders.set(folderPath, tree)
  return tree
}

/** Tree nodes for the loaded portion of the folder, in backend sort order. */
export function nodesFor(folderPath: string): ExplorerNode[] {
  const tree = getFolderTree(folderPath)
  // `index` is depth-first and parent-before-child, the order `FileTreeNode`
  // requires; it is handed out here rather than by `buildFileTree` because
  // these rows come straight from the backend listings.
  let next = 0
  const childNodes = (dirPath: string): ExplorerNode[] =>
    (tree.children.get(dirPath) ?? []).map((entry) => {
      const index = next++
      return {
        name: entry.name,
        path: entry.path,
        type: entry.is_dir ? ('directory' as const) : ('file' as const),
        index,
        lowerName: entry.name.toLowerCase(),
        children: entry.is_dir ? childNodes(entry.path) : [],
        change: entry.is_dir ? undefined : { path: entry.path }
      }
    })
  return childNodes('')
}

/** Paths the explorer should render as second-class: git-ignored or excluded. */
export function dimmedPathsFor(folderPath: string): Set<string> {
  const dimmed = new Set<string>()
  for (const entries of getFolderTree(folderPath).children.values()) {
    for (const entry of entries) {
      if (entry.ignored || entry.excluded) dimmed.add(entry.path)
    }
  }
  return dimmed
}

export function isExpanded(folderPath: string, dirPath: string): boolean {
  return getFolderTree(folderPath).expanded.has(dirPath)
}

function inflightKey(folderPath: string, dirPath: string): string {
  return `${folderPath}\u0000${dirPath}`
}

export function loadDir(folderPath: string, dirPath: string, force = false): Promise<void> {
  const tree = folderTreeFor(folderPath)
  const key = inflightKey(folderPath, dirPath)
  const existing = inflight.get(key)
  if (existing && !force) return existing
  if (!force && tree.children.has(dirPath)) return Promise.resolve()

  let task: Promise<void>
  // A forced read chains after the in-flight one so a caller that just mutated
  // the directory never resolves against the pre-mutation listing.
  task = (existing ?? Promise.resolve())
    .then(async () => {
      try {
        const entries = await backend.files.readDir(folderPath, dirPath, tree.showHidden)
        tree.children.set(dirPath, entries)
        tree.error = null
        scheduleWatchSync(folderPath)
      } catch (e) {
        const message = e instanceof Error ? e.message : String(e)
        if (dirPath === '') {
          tree.error = message
        } else {
          // A subdirectory that can no longer be read is usually one that was
          // just deleted; its row disappears with its parent's next listing.
          tree.children.delete(dirPath)
          console.warn(`Failed to read ${dirPath}:`, message)
        }
      }
    })
    .finally(() => {
      if (inflight.get(key) === task) inflight.delete(key)
    })
  inflight.set(key, task)
  return task
}

export function expandDir(folderPath: string, dirPath: string): void {
  const tree = folderTreeFor(folderPath)
  if (tree.expanded.has(dirPath)) return
  tree.expanded.add(dirPath)
  void loadDir(folderPath, dirPath)
}

export function toggleDir(folderPath: string, dirPath: string): void {
  const tree = folderTreeFor(folderPath)
  if (tree.expanded.has(dirPath)) {
    tree.expanded.delete(dirPath)
    scheduleWatchSync(folderPath)
    return
  }
  tree.expanded.add(dirPath)
  // A cached listing may predate the collapse, and nothing watched it while it
  // was hidden, so re-read rather than trust it.
  void loadDir(folderPath, dirPath, tree.children.has(dirPath))
}

/**
 * Expand and load every ancestor of `filePath` so the active file is visible.
 * Compacted rows span several segments, so ancestors are matched by path
 * prefix rather than by walking segments. Stops silently when a step is
 * missing, which is what happens for a file under an excluded directory.
 */
export async function revealPath(folderPath: string, filePath: string): Promise<void> {
  const tree = folderTreeFor(folderPath)
  await loadDir(folderPath, '')

  let dirPath = ''
  for (;;) {
    const next = tree.children.get(dirPath)?.find((entry) => entry.is_dir && isEqualOrParent(entry.path, filePath))
    if (!next) return
    tree.expanded.add(next.path)
    await loadDir(folderPath, next.path)
    dirPath = next.path
  }
}

export function setShowHidden(folderPath: string, showHidden: boolean): void {
  const tree = folderTreeFor(folderPath)
  if (tree.showHidden === showHidden) return
  tree.showHidden = showHidden
  void refreshFolderTree(folderPath)
}

/** Re-read every listing this folder currently holds. */
export function refreshFolderTree(folderPath: string): Promise<void> {
  const tree = folders.get(folderPath)
  if (!tree) return Promise.resolve()
  return Promise.all([...tree.children.keys()].map((dir) => loadDir(folderPath, dir, true))).then(() => undefined)
}

/**
 * Re-read just these listings; unloaded directories need no work. A directory
 * swallowed by a compacted row is not a listing of its own, so its change is
 * routed to the listing holding that row — re-reading it is what splits the
 * row apart.
 */
export function invalidateDirs(folderPath: string, dirs: string[]): Promise<void> {
  const tree = folders.get(folderPath)
  if (!tree) return Promise.resolve()
  const owners = hopOwners(tree)
  const listings = new Set<string>()
  for (const dir of dirs) {
    if (tree.children.has(dir)) listings.add(dir)
    const owner = owners.get(dir)
    if (owner !== undefined) listings.add(owner)
  }
  return Promise.all([...listings].map((dir) => loadDir(folderPath, dir, true))).then(() => undefined)
}

/** Each directory a compacted row swallowed -> the listing that row lives in. */
function hopOwners(tree: FolderTree): Map<string, string> {
  const owners = new Map<string, string>()
  for (const [listing, entries] of tree.children) {
    for (const entry of entries) {
      for (const hop of entry.hops) owners.set(hop, listing)
    }
  }
  return owners
}

export function releaseFileTree(folderPath: string): void {
  clearTimeout(watchTimers.get(folderPath))
  watchTimers.delete(folderPath)
  watchedSignatures.delete(folderPath)
  folders.delete(folderPath)
}

/**
 * Watch the root, every expanded directory that has a listing, and every
 * directory a compacted row swallowed — exactly what is on screen, plus the
 * hidden links whose contents decide how those rows collapse. Watching
 * recursively instead would spend thousands of inotify watches on trees the
 * user cannot see.
 */
function scheduleWatchSync(folderPath: string): void {
  clearTimeout(watchTimers.get(folderPath))

  watchTimers.set(
    folderPath,
    setTimeout(() => {
      watchTimers.delete(folderPath)
      const tree = folders.get(folderPath)
      if (!tree) return
      const dirs = [...['', ...tree.expanded].filter((dir) => tree.children.has(dir)), ...hopOwners(tree).keys()]
      // A refresh reloads every listing without changing the set, so skip the
      // round trip when the watcher would be told exactly what it holds.
      const signature = dirs.join('\u0000')
      if (watchedSignatures.get(folderPath) === signature) return
      watchedSignatures.set(folderPath, signature)
      backend.files.watchDirs(folderPath, dirs).catch((e) => console.warn('Explorer watch sync failed:', e))
    }, WATCH_SYNC_DELAY_MS)
  )
}

let listenersBooted = false

/** Boots the process-wide freshness listeners; safe to call repeatedly. */
export function ensureFileTreeListeners(): void {
  if (listenersBooted) return
  listenersBooted = true

  void backend.files.onChanged((event) => {
    void invalidateDirs(event.folder_path, event.dirs)
  })

  // Exclude globs, gitignore handling and compaction all live in settings, and
  // any of them can change what a listing contains.
  void backend.settings.onChanged(() => {
    for (const folderPath of [...folders.keys()]) {
      void refreshFolderTree(folderPath)
      markProjectFilesStale(folderPath)
    }
  })

  // Safety net for the file events Linux watchers miss, mirroring VS Code's
  // refresh on window focus.
  void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
    if (!focused) return
    for (const folderPath of [...folders.keys()]) void refreshFolderTree(folderPath)
  })
}
