// Per-file Monaco model + view-state cache.
//
// Models (ITextModel) are heavy: tokenization, worker, text buffer.
// Keeping one original+modified pair alive for every file in a large
// diff trips Monaco's internal listener leak heuristic long before the
// app is actually leaking. We therefore cache metadata, lazily loaded
// content + view state for every row, but materialize Monaco models
// only while that row's diff body is mounted.
//
// Ownership (mirrors VSCode's MultiDiffEditor):
//   - Models owned here, not by the pool.
//   - `sync(files)` reconciles metadata for every file.
//   - `retain(path)` loads content and creates models on demand.
//   - `release(path)` disposes models once no body retains them.
//   - `setViewState` / `setHeight` survive mount/unmount via this cache.

import type { DiffFileContent, FileDiff } from '$lib/types/backend'

type Monaco = typeof import('monaco-editor')
type ITextModel = import('monaco-editor').editor.ITextModel
type IDiffEditorViewState = import('monaco-editor').editor.IDiffEditorViewState

/** Per-file content fetcher. Returns the same shape the
 *  backend's per-file content endpoint produces (`DiffFileContent`). */
export type DiffContentFetcher = (entry: DiffModelEntry) => Promise<DiffFileContent>

export interface DiffModelEntry {
  /** File identity; primary map key. */
  path: string
  /** Cached pre-change content. `''` when the side is absent. */
  originalContent: string
  /** Cached post-change content. `''` when the side is absent. */
  modifiedContent: string
  /** Pre-change side. `null` for additions and untracked files. */
  original: ITextModel | null
  /** Post-change side. `null` for deletions. */
  modified: ITextModel | null
  /** Monaco language id resolved server-side. */
  lang: string
  /** Binary / oversized; row should render a placeholder instead. */
  binary: boolean
  /** Last saved view state (selections, scroll). Restored on re-acquire. */
  viewState: IDiffEditorViewState | null
  /** Last measured content height so re-mount doesn't trigger a jump. */
  height: number | null
  /**
   * Per-file override for Monaco's `hideUnchangedRegions`. Defaults to
   * `true`; unchanged code is collapsed with inline "show more"
   * affordances. Toggled by the per-row "expand all code" button.
   */
  hideUnchanged: boolean
  /** Numstat for the header. */
  additions: number | null
  deletions: number | null
  /** Status for the header badge. */
  status: FileDiff['status']
  /** Rename/copy old path for display. */
  oldPath: string | null
  /** Mounted diff bodies currently using these models. */
  retainCount: number
  /**
   * Entry left the active file set (or the whole store is tearing
   * down). Keep it around until the final retainer unbinds Monaco;
   * disposing the TextModels earlier trips Monaco's diff-editor
   * dispose-ordering invariant.
   */
  pendingRemoval: boolean
  /**
   * `true` once the entry has real content (or we know it's binary).
   * `false` until the per-file content loader resolves.
   */
  contentLoaded: boolean
  /**
   * Monotonic clock value updated on every `retain` / `get` / content
   * load. Drives LRU eviction when the entry count exceeds
   * [`MAX_LIVE_ENTRIES`].
   */
  lastAccess: number
}

/**
 * Soft cap on the number of `DiffModelEntry` instances kept in memory.
 * Each entry holds metadata + cached content strings (capped at the
 * server's `MAX_CONTENT_BYTES` per side). When the count exceeds this,
 * `sync` evicts the least-recently-used non-retained entries until
 * back under the cap. Picked to comfortably cover even pathological
 * commits (1000+ files) without unbounded growth on long review
 * sessions that walk through many separate diffs.
 */
const MAX_LIVE_ENTRIES = 500

export class DiffModelStore {
  private entries = new Map<string, DiffModelEntry>()
  private monaco: Monaco | null = null
  // In-flight content loads keyed by path: dedupes concurrent requests.
  private inflight = new Map<string, Promise<void>>()
  // Monotonic access clock for LRU eviction.
  private accessClock = 0
  // Paths in the current `sync(files)` set. Used by `enforceCapacity`
  // to refuse evicting any entry the UI still expects to render.
  private currentPaths = new Set<string>()
  /**
   * Reactive bump counter. Incremented whenever a lazy content load
   * resolves so consumer `$effect`s (e.g. `MonacoDiffBody`'s mount
   * effect) re-fire and pick up the now-ready models.
   */
  version = $state(0)

  /** Attach the Monaco module once it's loaded. Idempotent. */
  attach(monaco: Monaco): void {
    this.monaco = monaco
  }

  constructor(private readonly fetcher: DiffContentFetcher) {}

  /**
   * Reconcile metadata against a new file list without changing cached
   * contents or binary state. Reconcile language on live Monaco models.
   */
  sync(files: FileDiff[]): void {
    const nextPaths = new Set(files.map((f) => f.path))
    this.currentPaths = nextPaths

    // Stale rows may still have a mounted diff body or a queued height
    // preload retaining their models. Mark them for removal now, but
    // defer the actual model disposal until the last retainer releases.
    for (const [path, entry] of this.entries.entries()) {
      if (!nextPaths.has(path)) {
        this.retire(path, entry)
      }
    }

    for (const file of files) {
      const existing = this.entries.get(file.path)
      if (existing) {
        existing.pendingRemoval = false
        this.applyMetadata(existing, file)
        continue
      }
      this.entries.set(file.path, this.build(file))
    }

    // Cap memory growth on long review sessions: if the entry map
    // outgrew `MAX_LIVE_ENTRIES` (e.g. user walked through several
    // 200-file commits), evict least-recently-used non-retained
    // entries until back under the cap.
    this.enforceCapacity()
  }

  get(path: string): DiffModelEntry | null {
    const entry = this.entries.get(path)
    if (entry) entry.lastAccess = ++this.accessClock
    return entry ?? null
  }

  /**
   * Ensure a path has live Monaco models and mark them in-use. Mounted
   * diff bodies call this on attach and pair it with `release(path)` on
   * teardown so large diffs do not keep hundreds of dormant models live.
   *
   * Without loaded content, kicks off the content loader in the
   * background. The returned entry has `original`/`modified` = null
   * until the load resolves; consumers re-evaluate when `version` bumps.
   */
  retain(path: string): DiffModelEntry | null {
    const entry = this.entries.get(path)
    if (!entry) return null
    entry.retainCount += 1
    entry.lastAccess = ++this.accessClock
    if (entry.binary) return entry
    if (!entry.contentLoaded) {
      this.kickoffContentLoad(entry)
      return entry
    }
    this.ensureModels(entry)
    return entry
  }

  release(path: string): void {
    const entry = this.entries.get(path)
    if (!entry) return
    if (entry.retainCount > 0) {
      entry.retainCount -= 1
    }
    if (entry.retainCount === 0) {
      this.disposeModels(entry)
      if (entry.pendingRemoval) {
        this.entries.delete(path)
      }
    }
  }

  setViewState(path: string, state: IDiffEditorViewState | null): void {
    const e = this.entries.get(path)
    if (e) e.viewState = state
  }

  setHeight(path: string, height: number): void {
    const e = this.entries.get(path)
    if (e) e.height = height
  }

  /**
   * Persist the "hide unchanged regions" preference for a path so that
   * a row remount (row collapsed then re-expanded, or file briefly
   * leaves then re-enters the viewport) picks up the user's last
   * choice instead of resetting to the default. The live row keeps
   * its own `$state` mirror; this is the cross-mount cache only.
   */
  persistHideUnchangedPreference(path: string, hide: boolean): void {
    const e = this.entries.get(path)
    if (e) e.hideUnchanged = hide
  }

  /** Dispose every model. Call on viewer teardown / folder switch. */
  dispose(): void {
    for (const [path, entry] of this.entries) {
      this.retire(path, entry)
    }
  }

  private build(file: FileDiff): DiffModelEntry {
    return {
      path: file.path,
      originalContent: '',
      modifiedContent: '',
      original: null,
      modified: null,
      lang: file.lang,
      binary: false,
      viewState: null,
      height: null,
      hideUnchanged: true,
      additions: file.additions,
      deletions: file.deletions,
      status: file.status,
      oldPath: file.oldPath,
      retainCount: 0,
      pendingRemoval: false,
      contentLoaded: false,
      lastAccess: ++this.accessClock
    }
  }

  /**
   * Patch metadata without touching loaded content or binary state.
   * Reconcile language while live Monaco models exist.
   */
  private applyMetadata(entry: DiffModelEntry, file: FileDiff): void {
    entry.lang = file.lang
    entry.additions = file.additions
    entry.deletions = file.deletions
    entry.status = file.status
    entry.oldPath = file.oldPath

    if (!entry.binary && entry.contentLoaded && (entry.retainCount > 0 || entry.original || entry.modified)) {
      this.ensureModels(entry)
    }
  }

  private ensureModels(entry: DiffModelEntry): void {
    if (!this.monaco || entry.binary) return
    const m = this.monaco
    entry.original = this.reconcileSide(entry.original, entry.originalContent, entry.lang, m)
    entry.modified = this.reconcileSide(entry.modified, entry.modifiedContent, entry.lang, m)
  }

  private disposeModels(entry: DiffModelEntry): void {
    entry.original?.dispose()
    entry.modified?.dispose()
    entry.original = null
    entry.modified = null
  }

  private retire(path: string, entry: DiffModelEntry): void {
    entry.pendingRemoval = true
    if (entry.retainCount > 0) return
    this.disposeModels(entry)
    this.entries.delete(path)
  }

  private reconcileSide(current: ITextModel | null, next: string, lang: string, m: Monaco): ITextModel {
    if (!current) {
      return m.editor.createModel(next, lang)
    }
    if (current.getValue() !== next) {
      current.setValue(next)
    }
    if (current.getLanguageId() !== lang) {
      m.editor.setModelLanguage(current, lang)
    }
    return current
  }

  /**
   * If the entry map exceeds [`MAX_LIVE_ENTRIES`], evict the oldest
   * non-retained entries (ordered by `lastAccess` ascending) until
   * back under the cap. Two exclusions:
   *   1. Retained entries (a row, preloader, or queued measurement is
   *      actively using them; disposing leaves dangling Monaco refs).
   *   2. Paths in the current `sync(files)` set. Without this guard a
   *      diff over `MAX_LIVE_ENTRIES` files would evict entries the UI
   *      still wants to render, leaving blank rows when the user
   *      scrolls past the viewport's retained set.
   */
  private enforceCapacity(): void {
    if (this.entries.size <= MAX_LIVE_ENTRIES) return
    const candidates: Array<{ path: string; entry: DiffModelEntry }> = []
    for (const [path, entry] of this.entries) {
      if (entry.retainCount === 0 && !this.currentPaths.has(path)) {
        candidates.push({ path, entry })
      }
    }
    candidates.sort((a, b) => a.entry.lastAccess - b.entry.lastAccess)
    let toEvict = this.entries.size - MAX_LIVE_ENTRIES
    for (const { path, entry } of candidates) {
      if (toEvict <= 0) break
      this.disposeModels(entry)
      this.entries.delete(path)
      toEvict -= 1
    }
  }

  /**
   * Kick off a per-file content load. No-op when content is already
   * loaded or a load is in-flight. On completion, bumps the store's
   * version so consumers pick up loaded contents and binary state.
   */
  private kickoffContentLoad(entry: DiffModelEntry): void {
    if (entry.contentLoaded) return
    if (this.inflight.has(entry.path)) return

    const promise = (async () => {
      try {
        const result = await this.fetcher(entry)
        // Entry may have been retired while we were awaiting.
        if (!this.entries.has(entry.path)) return
        entry.contentLoaded = true
        entry.lastAccess = ++this.accessClock
        if (result.binary) {
          entry.binary = true
          if (entry.retainCount === 0) this.disposeModels(entry)
        } else {
          entry.binary = false
          entry.originalContent = result.oldContent ?? ''
          entry.modifiedContent = result.newContent ?? ''
          if (entry.retainCount > 0) {
            this.ensureModels(entry)
          }
        }
      } catch (err) {
        console.error('Diff content load failed for', entry.path, err)
      } finally {
        this.inflight.delete(entry.path)
        // Bump unconditionally so the consumer effect re-runs even when
        // the fetcher threw. Otherwise a row whose load failed stays
        // parked at the placeholder height with no Monaco bound.
        this.version++
      }
    })()
    this.inflight.set(entry.path, promise)
  }
}
