// Per-file Monaco model + view-state cache.
//
// Models (ITextModel) are heavy — tokenization, worker, text buffer.
// We create them once per file and reuse across mount/unmount so the
// pool swaps them into a DiffEditor without re-tokenizing.
//
// Ownership (mirrors VSCode's MultiDiffEditor):
//   - Models owned here, not by the pool.
//   - `sync(files)` reconciles: dispose missing, create new, setValue existing.
//   - `setViewState` / `setHeight` survive mount/unmount via this cache.

import type { FileDiff } from '$lib/types/backend'

type Monaco = typeof import('monaco-editor')
type ITextModel = import('monaco-editor').editor.ITextModel
type IDiffEditorViewState = import('monaco-editor').editor.IDiffEditorViewState

export interface DiffModelEntry {
  /** File identity — primary map key. */
  path: string
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
}

export class DiffModelStore {
  private entries = new Map<string, DiffModelEntry>()
  private monaco: Monaco | null = null

  /** Attach the Monaco module once it's loaded. Idempotent. */
  attach(monaco: Monaco): void {
    this.monaco = monaco
  }

  /**
   * Reconcile the model set against a new file list. Disposes models
   * whose path left the set, creates models for new paths, updates
   * content on paths whose content changed (cheap: `setValue()` keeps
   * the ITextModel identity and preserves reactive bindings).
   */
  sync(files: FileDiff[]): void {
    if (!this.monaco) return
    const m = this.monaco
    const nextPaths = new Set(files.map((f) => f.path))

    // Dispose stale entries first — models left out of the new set.
    for (const [path, entry] of this.entries) {
      if (!nextPaths.has(path)) {
        entry.original?.dispose()
        entry.modified?.dispose()
        this.entries.delete(path)
      }
    }

    for (const file of files) {
      const existing = this.entries.get(file.path)
      if (existing) {
        // Same path: refresh content in place. Cheaper than disposing
        // and recreating — preserves decorations, undo, bookmarks.
        this.applyContent(existing, file, m)
        continue
      }
      this.entries.set(file.path, this.build(file, m))
    }
  }

  get(path: string): DiffModelEntry | null {
    return this.entries.get(path) ?? null
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
      entry.original?.dispose()
      entry.modified?.dispose()
    }
    this.entries.clear()
  }

  private build(file: FileDiff, m: Monaco): DiffModelEntry {
    // Binary / oversized files skip Monaco entirely — the UI renders a
    // placeholder. Leaving models `null` signals that to the row.
    if (file.binary) {
      return {
        path: file.path,
        original: null,
        modified: null,
        lang: file.lang,
        binary: true,
        viewState: null,
        height: null,
        hideUnchanged: true,
        additions: file.additions,
        deletions: file.deletions,
        status: file.status,
        oldPath: file.oldPath
      }
    }
    // For text diffs we always build BOTH models — one-sided adds/deletes
    // use an empty string for the missing side so the diff editor can
    // render the whole content as added/deleted lines naturally.
    return {
      path: file.path,
      original: m.editor.createModel(file.oldContent ?? '', file.lang),
      modified: m.editor.createModel(file.newContent ?? '', file.lang),
      lang: file.lang,
      binary: false,
      viewState: null,
      height: null,
      hideUnchanged: true,
      additions: file.additions,
      deletions: file.deletions,
      status: file.status,
      oldPath: file.oldPath
    }
  }

  /**
   * Patch an existing entry's content + metadata. Keeps ITextModel
   * identity so any editor currently bound to them doesn't notice.
   * Recreates a side when it transitions null ↔ string (e.g. a file
   * that was added is now modified, so `original` appears).
   */
  private applyContent(entry: DiffModelEntry, file: FileDiff, m: Monaco): void {
    entry.lang = file.lang
    entry.additions = file.additions
    entry.deletions = file.deletions
    entry.status = file.status
    entry.oldPath = file.oldPath

    // Transition into or out of "binary" — if the new file is binary
    // dispose any live models; if the file flipped back to text, new
    // models are created by `reconcileSide` below.
    if (file.binary) {
      entry.original?.dispose()
      entry.modified?.dispose()
      entry.original = null
      entry.modified = null
      entry.binary = true
      return
    }
    entry.binary = false
    entry.original = this.reconcileSide(entry.original, file.oldContent ?? '', file.lang, m)
    entry.modified = this.reconcileSide(entry.modified, file.newContent ?? '', file.lang, m)
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
