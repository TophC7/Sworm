// Per-file Monaco model + view-state cache.
//
// Models (ITextModel) are heavy — tokenization, worker, text buffer.
// Keeping one original+modified pair alive for every file in a large
// diff trips Monaco's internal listener leak heuristic long before the
// app is actually leaking. We therefore cache file data + view state
// for every row, but materialize Monaco models only while that row's
// diff body is mounted.
//
// Ownership (mirrors VSCode's MultiDiffEditor):
//   - Models owned here, not by the pool.
//   - `sync(files)` reconciles metadata + content for every file.
//   - `retain(path)` / `release(path)` lazily create and dispose models.
//   - `setViewState` / `setHeight` survive mount/unmount via this cache.

import type { FileDiff } from '$lib/types/backend'

type Monaco = typeof import('monaco-editor')
type ITextModel = import('monaco-editor').editor.ITextModel
type IDiffEditorViewState = import('monaco-editor').editor.IDiffEditorViewState

export interface DiffModelEntry {
  /** File identity — primary map key. */
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
  /** Binary / oversized — row should render a placeholder instead. */
  binary: boolean
  /** Last saved view state (selections, scroll). Restored on re-acquire. */
  viewState: IDiffEditorViewState | null
  /** Last measured content height so re-mount doesn't trigger a jump. */
  height: number | null
  /**
   * Per-file override for Monaco's `hideUnchangedRegions`. Defaults to
   * `true` — unchanged code is collapsed with inline "show more"
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
}

export class DiffModelStore {
  private entries = new Map<string, DiffModelEntry>()
  private monaco: Monaco | null = null

  /** Attach the Monaco module once it's loaded. Idempotent. */
  attach(monaco: Monaco): void {
    this.monaco = monaco
  }

  /**
   * Reconcile the entry set against a new file list. Metadata and raw
   * content are always updated; Monaco models are patched only while a
   * row actively retains them.
   */
  sync(files: FileDiff[]): void {
    const nextPaths = new Set(files.map((f) => f.path))

    // Dispose stale entries first — rows left out of the new set.
    for (const [path, entry] of this.entries) {
      if (!nextPaths.has(path)) {
        this.disposeModels(entry)
        this.entries.delete(path)
      }
    }

    for (const file of files) {
      const existing = this.entries.get(file.path)
      if (existing) {
        this.applyContent(existing, file)
        continue
      }
      this.entries.set(file.path, this.build(file))
    }
  }

  get(path: string): DiffModelEntry | null {
    return this.entries.get(path) ?? null
  }

  /**
   * Ensure a path has live Monaco models and mark them in-use. Mounted
   * diff bodies call this on attach and pair it with `release(path)` on
   * teardown so large diffs do not keep hundreds of dormant models live.
   */
  retain(path: string): DiffModelEntry | null {
    const entry = this.entries.get(path)
    if (!entry) return null
    if (!entry.binary) {
      entry.retainCount += 1
      this.ensureModels(entry)
    }
    return entry
  }

  release(path: string): void {
    const entry = this.entries.get(path)
    if (!entry || entry.binary) return
    if (entry.retainCount > 0) {
      entry.retainCount -= 1
    }
    if (entry.retainCount === 0) {
      this.disposeModels(entry)
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
   * its own `$state` mirror — this is the cross-mount cache only.
   */
  persistHideUnchangedPreference(path: string, hide: boolean): void {
    const e = this.entries.get(path)
    if (e) e.hideUnchanged = hide
  }

  /** Dispose every model. Call on viewer teardown / project switch. */
  dispose(): void {
    for (const entry of this.entries.values()) {
      this.disposeModels(entry)
    }
    this.entries.clear()
  }

  private build(file: FileDiff): DiffModelEntry {
    return {
      path: file.path,
      originalContent: file.oldContent ?? '',
      modifiedContent: file.newContent ?? '',
      original: null,
      modified: null,
      lang: file.lang,
      binary: file.binary,
      viewState: null,
      height: null,
      hideUnchanged: true,
      additions: file.additions,
      deletions: file.deletions,
      status: file.status,
      oldPath: file.oldPath,
      retainCount: 0
    }
  }

  /**
   * Patch an existing entry's metadata + cached content. Live Monaco
   * models are updated only while a mounted diff body retains them.
   */
  private applyContent(entry: DiffModelEntry, file: FileDiff): void {
    entry.lang = file.lang
    entry.additions = file.additions
    entry.deletions = file.deletions
    entry.status = file.status
    entry.oldPath = file.oldPath
    entry.originalContent = file.oldContent ?? ''
    entry.modifiedContent = file.newContent ?? ''

    if (file.binary) {
      entry.binary = true
      if (entry.retainCount === 0) {
        this.disposeModels(entry)
      }
      return
    }

    entry.binary = false
    if (entry.retainCount > 0 || entry.original || entry.modified) {
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
}
