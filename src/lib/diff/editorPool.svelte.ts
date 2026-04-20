// Monaco DiffEditor pool — port of VSCode's MultiDiffEditor ObjectPool.
// Caps unused editors at MAX_UNUSED; used grows with visible rows.
//
// Invariants:
//   - Each editor lives in its OWN stable <div>, reparented wholesale
//     to the row host on acquire. Monaco's internal DOM never moves —
//     reparenting it surfaces as "Illegal value for lineNumber".
//   - Unused editors park off-screen (not display:none — that
//     de-initializes Monaco's layout).
//   - Focused editors defer release until blur (yanking state mid-IME
//     loses composition).
//   - Models are owned by DiffModelStore; the pool only `setModel`s them.
//   - View-state is saved before release, restored on sticky reuse.

import { SWORM_THEME_NAME } from '$lib/editor/monacoTheme'

type Monaco = typeof import('monaco-editor')
type DiffEditor = import('monaco-editor').editor.IStandaloneDiffEditor
type DiffOptions = import('monaco-editor').editor.IDiffEditorOptions & import('monaco-editor').editor.IEditorOptions

/** Options that drive the editor look/behavior. Changes propagate to every live editor. */
export interface DiffEditorSettings {
  renderSideBySide: boolean
  wordWrap: boolean
  fontSize: number
}

export interface PoolRef {
  /** Underlying Monaco instance. */
  editor: DiffEditor
  /**
   * Stable container. Monaco's internal DOM lives inside this div and
   * NEVER moves. The container itself gets appended to the row host on
   * acquire and back to parking on release — one node's worth of move
   * instead of potentially hundreds.
   */
  container: HTMLDivElement
  /** `true` while the ref is checked out by a row. */
  inUse: boolean
  /** Internal: pool-only, last path this editor was bound to. */
  _lastPath: string | null
  /** Internal dispose: only called when exceeding pool cap. */
  destroy: () => void
}

// Monaco exposes its live unchanged-region model only as an internal.
// Narrow shape used by `hasExpandedUnchanged` to detect when the user
// has manually expanded any region via Monaco's own UI — which means
// the row toggle's live state has drifted from the persisted pref.
type DiffEditorInternal = DiffEditor & {
  _diffModel?: {
    get?: () =>
      | {
          unchangedRegions?: {
            get?: () => Array<{
              lineCount?: number
              modifiedUnchangedRange?: { startLineNumber?: number; endLineNumberExclusive?: number }
              getHiddenModifiedRange?: (reader?: unknown) => {
                startLineNumber?: number
                endLineNumberExclusive?: number
              }
            }>
          }
        }
      | undefined
  }
  showAllUnchangedRegions?: () => void
  collapseAllUnchangedRegions?: () => void
}

function getRangeLength(range: { startLineNumber?: number; endLineNumberExclusive?: number } | undefined): number {
  if (!range) return 0
  return Math.max(0, (range.endLineNumberExclusive ?? 0) - (range.startLineNumber ?? 0))
}

/** Result of {@link DiffEditorPool.acquire}. */
export interface AcquireResult {
  ref: PoolRef
  /**
   * `true` when the pool handed back the same editor that was last bound
   * to this path. Callers can restore cached view-state only on sticky
   * reuse — on a fresh editor the cache belongs to a different document.
   */
  stickyReuse: boolean
}

const MAX_UNUSED = 6

/**
 * Monaco's `hideUnchangedRegions` config — treated as a unit: passing
 * `{ enabled }` alone resets the tuning fields. Built via a factory so
 * the toggle can flip `enabled` without repeating the tuning everywhere.
 */
function hideUnchangedOpts(enabled: boolean) {
  return {
    enabled,
    contextLineCount: 3,
    minimumLineCount: 3,
    revealLineCount: 20
  }
}

/**
 * Off-screen parking area for pooled but unused editors. Must remain
 * in the document so Monaco's layout calculations stay valid; hiding
 * with `display: none` de-initializes Monaco. Instead we pin it far
 * off-screen with tiny dimensions.
 */
let parkingEl: HTMLDivElement | null = null
function getParking(): HTMLDivElement {
  if (!parkingEl) {
    parkingEl = document.createElement('div')
    parkingEl.setAttribute('aria-hidden', 'true')
    parkingEl.style.cssText = [
      'position:absolute',
      'left:-99999px',
      'top:-99999px',
      'width:800px',
      'height:400px',
      'overflow:hidden',
      'pointer-events:none',
      'visibility:hidden'
    ].join(';')
    document.body.appendChild(parkingEl)
  }
  return parkingEl
}

const UNUSED_TRIM_DELAY_MS = 1000

type IdleHandle = { dispose(): void }

function scheduleIdle(callback: () => void): IdleHandle {
  if (typeof window.requestIdleCallback === 'function') {
    const id = window.requestIdleCallback(callback, { timeout: UNUSED_TRIM_DELAY_MS })
    return { dispose: () => window.cancelIdleCallback(id) }
  }

  const id = window.setTimeout(callback, UNUSED_TRIM_DELAY_MS)
  return { dispose: () => window.clearTimeout(id) }
}

export class DiffEditorPool {
  private monacoPromise: Promise<Monaco> | null = null
  private unused: PoolRef[] = []
  private used: Set<PoolRef> = new Set()
  private trimHandle: IdleHandle | null = null
  private settings: DiffEditorSettings = {
    renderSideBySide: true,
    wordWrap: false,
    fontSize: 13
  }

  /** Lazy, cached Monaco module load. */
  async ready(): Promise<Monaco> {
    if (!this.monacoPromise) {
      this.monacoPromise = (async () => {
        // Both imports are dynamic so Monaco + the env bootstrap land
        // in a lazy chunk together, matching `MonacoEditor.svelte`.
        const [{ initMonaco }, m] = await Promise.all([import('$lib/editor/monacoEnv'), import('monaco-editor')])
        await initMonaco(m)
        // Apply the Sworm theme globally so every editor we construct
        // inherits it. `createDiffEditor` doesn't take a theme in its
        // options; this is the standard Monaco pattern.
        m.editor.setTheme(SWORM_THEME_NAME)
        return m
      })()
    }
    return this.monacoPromise
  }

  /**
   * Acquire a DiffEditor into `host`. Prefers an unused editor that was
   * last bound to `path` (VSCode's "sticky re-hydration" — avoids a
   * re-tokenize pass when a row briefly leaves then re-enters the
   * viewport). Falls back to any unused editor, then constructs a new
   * one if none are available.
   *
   * `stickyReuse` in the result is `true` iff the returned editor was
   * already bound to this path. Callers should only restore cached
   * view-state in that case.
   */
  async acquire(host: HTMLElement, path: string | null): Promise<AcquireResult> {
    const monaco = await this.ready()

    let ref: PoolRef | null = null
    let stickyReuse = false
    if (path) {
      const idx = this.unused.findIndex((r) => r._lastPath === path)
      if (idx >= 0) {
        ref = this.unused.splice(idx, 1)[0]
        stickyReuse = true
      }
    }
    if (!ref && this.unused.length > 0) {
      ref = this.unused.shift() as PoolRef
    }
    if (!ref) {
      ref = this.createRef(monaco)
    }

    // Move the stable container into the row host. Monaco's own DOM
    // (children of `ref.container`) is untouched — one node's worth of
    // move, not hundreds of Monaco internals.
    host.appendChild(ref.container)

    ref.inUse = true
    ref._lastPath = path
    this.used.add(ref)
    // Layout against the new host dimensions on the next frame. A
    // `requestAnimationFrame` beats `queueMicrotask` here — the browser
    // hasn't yet computed layout after the appendChild, so microtask-
    // time `clientWidth/Height` can read stale zeroes.
    requestAnimationFrame(() => {
      if (ref!.inUse) ref!.editor.layout()
    })
    return { ref, stickyReuse }
  }

  /**
   * Return an editor to the pool. If the editor has focus, the release
   * is deferred to the next blur — never yank state out from under an
   * active typist. Past the `MAX_UNUSED` cap, the editor is disposed
   * instead of warehoused.
   */
  release(ref: PoolRef): void {
    if (!this.used.has(ref)) return
    // The DiffEditor's focus state is split between its two inner
    // editors — check both sides before yanking state.
    const modified = ref.editor.getModifiedEditor()
    const original = ref.editor.getOriginalEditor()
    if (modified.hasTextFocus() || original.hasTextFocus()) {
      const offMod = modified.onDidBlurEditorText(() => {
        offMod.dispose()
        offOrig.dispose()
        this.release(ref)
      })
      const offOrig = original.onDidBlurEditorText(() => {
        offMod.dispose()
        offOrig.dispose()
        this.release(ref)
      })
      return
    }
    this.used.delete(ref)
    ref.inUse = false

    // Detach the model first. A pooled editor holding a model will
    // fire updates when the model's content changes, which can crash
    // the editor if the row's host has already been torn down.
    ref.editor.setModel(null)

    // Move the whole container back to parking BEFORE the row's host
    // gets destroyed by Svelte. Keeping the container in-document
    // preserves Monaco's layout state — `display: none` would trigger
    // a full layout invalidation on re-acquire.
    getParking().appendChild(ref.container)
    this.unused.push(ref)
    this.scheduleTrim()
  }

  /**
   * Flip `hideUnchangedRegions` on a live ref. `show*` / `collapseAll*`
   * drive the already-bound diff view (updateOptions alone doesn't
   * reliably transition it); the options update keeps parked editors
   * consistent for the next acquire.
   */
  applyHideUnchanged(ref: PoolRef, hide: boolean): void {
    const editor = ref.editor as DiffEditorInternal
    if (!hide) editor.showAllUnchangedRegions?.()
    editor.updateOptions({ hideUnchangedRegions: hideUnchangedOpts(hide) })
    if (hide) editor.collapseAllUnchangedRegions?.()
  }

  /**
   * Seed `hideUnchangedRegions` on a ref BEFORE binding models — used
   * during acquire so the first paint has the right feature state.
   */
  seedHideUnchanged(ref: PoolRef, hide: boolean): void {
    ref.editor.updateOptions({ hideUnchangedRegions: hideUnchangedOpts(hide) })
  }

  /**
   * Does the ref currently show ANY unchanged region that the `hide`
   * preference would otherwise collapse? `hide = false` → always true
   * (the preference itself is "show everything"). `hide = true` →
   * true when the user has manually expanded at least one region via
   * Monaco's own "Show X more lines" affordance, meaning the live
   * view has drifted from the preference.
   */
  hasExpandedUnchanged(ref: PoolRef, hide: boolean): boolean {
    if (!hide) return true
    const editor = ref.editor as DiffEditorInternal
    const regions = editor._diffModel?.get?.()?.unchangedRegions?.get?.() ?? []
    return regions.some((region) => {
      const hiddenLength = getRangeLength(region.getHiddenModifiedRange?.(undefined))
      const regionLength = region.lineCount ?? getRangeLength(region.modifiedUnchangedRange) ?? hiddenLength
      return hiddenLength < regionLength
    })
  }

  /**
   * Apply global settings (mode/wrap/font) to every live and pooled
   * editor so the switch is instantaneous and survives re-acquire.
   */
  updateSettings(patch: Partial<DiffEditorSettings>): void {
    this.settings = { ...this.settings, ...patch }
    const opts = this.buildOptions()
    for (const ref of this.used) ref.editor.updateOptions(opts)
    for (const ref of this.unused) ref.editor.updateOptions(opts)
  }

  getSettings(): Readonly<DiffEditorSettings> {
    return this.settings
  }

  /** Dispose every editor. Call on viewer teardown / project switch. */
  dispose(): void {
    this.trimHandle?.dispose()
    this.trimHandle = null
    for (const ref of this.used) ref.destroy()
    for (const ref of this.unused) ref.destroy()
    this.used.clear()
    this.unused = []
  }

  private createRef(monaco: Monaco): PoolRef {
    // Stable container. Lives in the parking area until the first
    // acquire moves it into a real row host.
    const container = document.createElement('div')
    container.style.cssText = 'width:100%;height:100%;'
    getParking().appendChild(container)

    const editor = monaco.editor.createDiffEditor(container, this.buildOptions() as DiffOptions)
    const ref: PoolRef = {
      editor,
      container,
      _lastPath: null,
      inUse: false,
      destroy: () => {
        editor.setModel(null)
        editor.dispose()
        container.remove()
      }
    }
    return ref
  }

  private buildOptions(): DiffOptions {
    return {
      renderSideBySide: this.settings.renderSideBySide,
      wordWrap: this.settings.wordWrap ? 'on' : 'off',
      fontSize: this.settings.fontSize,
      // The outer scroll container owns Y — per-row internal scrollbars
      // are hidden and wheel events bubble up. Matches VSCode's
      // multi-diff scroll forwarding.
      scrollbar: {
        vertical: 'hidden',
        horizontal: 'auto',
        handleMouseWheel: false,
        verticalScrollbarSize: 0,
        horizontalScrollbarSize: 6,
        useShadows: false,
        alwaysConsumeMouseWheel: false
      },
      scrollBeyondLastLine: false,
      automaticLayout: false,
      // Collapses unchanged regions with inline "Show X more lines"
      // affordances. For pure adds/deletes there are no unchanged
      // regions, so this is a no-op.
      hideUnchangedRegions: hideUnchangedOpts(true),
      renderOverviewRuler: false,
      overviewRulerBorder: false,
      overviewRulerLanes: 0,
      lineNumbers: 'on',
      glyphMargin: false,
      folding: false,
      fontFamily: "'Monocraft Nerd Font', monospace",
      fontLigatures: false,
      lineHeight: 20,
      minimap: { enabled: false },
      smoothScrolling: true,
      padding: { top: 0, bottom: 0 },
      renderLineHighlight: 'none',
      selectionHighlight: false,
      occurrencesHighlight: 'off',
      quickSuggestions: false,
      suggestOnTriggerCharacters: false,
      parameterHints: { enabled: false },
      contextmenu: true,
      // DiffEditor-specific:
      originalEditable: false,
      // This is a viewer, not an editor. Leaving the modified side writable
      // causes Monaco to spin up edit-only contributions like SuggestWidget,
      // which can race with pool disposal during collapse-all / trim bursts.
      readOnly: true,
      renderMarginRevertIcon: false,
      diffWordWrap: this.settings.wordWrap ? 'on' : 'off',
      enableSplitViewResizing: false
    }
  }

  private scheduleTrim(): void {
    if (this.unused.length <= MAX_UNUSED || this.trimHandle) return

    this.trimHandle = scheduleIdle(() => {
      this.trimHandle = null
      while (this.unused.length > MAX_UNUSED) {
        const ref = this.unused.shift()
        ref?.destroy()
      }
    })
  }
}

// Module-level singleton. One pool per app is sufficient — Sworm only
// shows one multi-file diff at a time.
let poolInstance: DiffEditorPool | null = null

export function getDiffEditorPool(): DiffEditorPool {
  if (!poolInstance) poolInstance = new DiffEditorPool()
  return poolInstance
}

export function disposeDiffEditorPool(): void {
  poolInstance?.dispose()
  poolInstance = null
}
