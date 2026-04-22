// Off-screen height pre-computation for diff rows.
//
// The visible-row DiffEditor can't know the post-`hideUnchangedRegions`
// row height until its diff worker has run AND the hideUnchanged widgets
// have materialized. Until then, a content-height measurement returns
// the full modified-file height — which causes the row to grow to the
// full file, then shrink back.
//
// Mirrors VSCode's trick for MultiDiffEditor: they eagerly construct
// `DiffEditorViewModel` for every file when the multi-diff opens, so by
// the time a row scrolls into view the diff is already computed. The
// standalone Monaco API doesn't expose `setDiffModel(viewModel)`, so we
// simulate it with a single hidden DiffEditor that runs through the file
// queue: bind models → wait for `onDidUpdateDiff` + a couple frames →
// read `getContentHeight()` → persist to `DiffModelStore.height`.
//
// A row that mounts with a cached `entry.height` seeds its host div at
// the right size, so the shift reduces to at most one pixel-level
// adjustment when the live editor's widgets materialize.

import {
  createDiffOptions,
  type DiffEditorSettings
} from '$lib/features/editor/renderers/monaco/diff/editorPool.svelte'
import { SWORM_THEME_NAME } from '$lib/features/editor/renderers/monaco/core/monacoTheme'
import type { DiffModelStore } from '$lib/features/workbench/surfaces/diff/diffModels.svelte'

type Monaco = typeof import('monaco-editor')
type DiffEditor = import('monaco-editor').editor.IStandaloneDiffEditor
type DiffOptions = import('monaco-editor').editor.IDiffEditorOptions & import('monaco-editor').editor.IEditorOptions

export interface PreloadParams {
  monaco: Monaco
  store: DiffModelStore
  path: string
  settings: DiffEditorSettings
}

interface QueueItem extends PreloadParams {}

// Fallback seed if the measure pipeline fails. Matches VSCode's default
// placeholder so first-paint sizes are in the same ballpark.
const DEFAULT_HEIGHT = 500
// Number of frames to wait after `onDidUpdateDiff` before measuring.
// Ensures `hideUnchangedRegions` widgets have been applied by the layout
// pipeline — without this, `getContentHeight()` still returns the
// pre-collapse height.
const POST_DIFF_FRAMES = 2
// Hard cap on any single measure to avoid a wedged worker holding the
// queue forever.
const MEASURE_TIMEOUT_MS = 1500
// Hidden container dimensions. Width must be wide enough that typical
// diff lines don't wrap at the measurement width but not at the real
// viewport — VSCode uses ~1000px for the same reason.
const HIDDEN_WIDTH = 1000
const HIDDEN_HEIGHT = 600

export class DiffHeightPreloader {
  private editor: DiffEditor | null = null
  private container: HTMLDivElement | null = null
  private queue: QueueItem[] = []
  private queued = new Set<string>()
  private processing = false
  private disposed = false
  private currentSettings: DiffEditorSettings | null = null

  /**
   * Queue a path for height pre-computation. De-duped by path — a
   * second enqueue while an earlier one is still pending is a no-op.
   * Resolved height is written back to `params.store` via `setHeight`;
   * fire-and-forget from the caller's perspective.
   */
  preload(params: PreloadParams): void {
    if (this.disposed) return
    if (this.queued.has(params.path)) return
    this.queued.add(params.path)
    this.ensure(params.monaco, params.settings)
    this.queue.push(params)
    this.pump()
  }

  /** Tear down the hidden editor. Call on viewer teardown. */
  dispose(): void {
    if (this.disposed) return
    this.disposed = true
    this.queue = []
    this.queued.clear()
    this.editor?.setModel(null)
    this.editor?.dispose()
    this.container?.remove()
    this.editor = null
    this.container = null
  }

  /**
   * Lazy-init the hidden editor + apply the latest settings. Safe to
   * call repeatedly; only the first call constructs the DOM + editor.
   */
  private ensure(monaco: Monaco, settings: DiffEditorSettings): void {
    if (this.disposed) return
    if (!this.editor) {
      const container = document.createElement('div')
      container.setAttribute('aria-hidden', 'true')
      container.style.cssText = [
        'position:absolute',
        // Off-screen but laid out — Monaco needs non-zero dimensions to
        // compute content height correctly.
        'left:-99999px',
        'top:-99999px',
        `width:${HIDDEN_WIDTH}px`,
        `height:${HIDDEN_HEIGHT}px`,
        'overflow:hidden',
        'pointer-events:none',
        'visibility:hidden'
      ].join(';')
      document.body.appendChild(container)
      this.container = container

      this.editor = monaco.editor.createDiffEditor(container, this.buildOptions(settings))
      this.currentSettings = settings
      monaco.editor.setTheme(SWORM_THEME_NAME)
      this.editor.layout({ width: HIDDEN_WIDTH, height: HIDDEN_HEIGHT })
      return
    }
    this.applySettings(settings)
  }

  private applySettings(settings: DiffEditorSettings): void {
    if (!this.editor) return
    if (
      this.currentSettings &&
      this.currentSettings.renderSideBySide === settings.renderSideBySide &&
      this.currentSettings.wordWrap === settings.wordWrap &&
      this.currentSettings.fontSize === settings.fontSize
    ) {
      return
    }
    this.currentSettings = settings
    this.editor.updateOptions(this.buildOptions(settings))
  }

  /**
   * Shared base + measurement-only overrides. Hidden scrollbars and no
   * contextmenu keep the hidden editor lean and ensure the measured
   * height matches what the live row will show once its internal
   * scrollbars are also hidden by the outer scroll container.
   */
  private buildOptions(settings: DiffEditorSettings): DiffOptions {
    const base = createDiffOptions(settings)
    return {
      ...base,
      scrollbar: {
        ...base.scrollbar,
        horizontal: 'hidden',
        horizontalScrollbarSize: 0
      },
      contextmenu: false,
      smoothScrolling: false
    }
  }

  private pump(): void {
    if (this.processing || this.disposed) return
    const next = this.queue.shift()
    if (!next) return
    this.processing = true
    this.measure(next).finally(() => {
      this.processing = false
      this.queued.delete(next.path)
      if (!this.disposed) this.pump()
    })
  }

  private async measure(item: QueueItem): Promise<void> {
    const editor = this.editor
    if (!editor || this.disposed) return

    this.applySettings(item.settings)

    // Retain models lazily — only for the measurement window. Retaining
    // every queued file up-front would hold N text-model pairs in memory
    // while the serial queue drains one at a time.
    const retained = item.store.retain(item.path)
    try {
      if (!retained || retained.binary || !retained.original || !retained.modified) return

      // Detach any prior model first so the diff worker starts fresh.
      editor.setModel(null)

      const h = await this.runMeasure(editor, retained.original, retained.modified)
      if (this.disposed) return
      if (h != null) item.store.setHeight(item.path, h)
    } finally {
      // Unbind BEFORE releasing the store retain. If this measure was the
      // sole retainer, `release()` will synchronously dispose the text
      // models — and Monaco throws "TextModel got disposed before
      // DiffEditorWidget model got reset" if the editor is still bound
      // when that happens.
      editor.setModel(null)
      item.store.release(item.path)
    }
  }

  private runMeasure(
    editor: DiffEditor,
    original: import('monaco-editor').editor.ITextModel,
    modified: import('monaco-editor').editor.ITextModel
  ): Promise<number | null> {
    return new Promise((resolve) => {
      let settled = false
      const finish = (value: number | null) => {
        if (settled) return
        settled = true
        off.dispose()
        window.clearTimeout(timer)
        resolve(value)
      }

      const off = editor.onDidUpdateDiff(() => {
        let framesLeft = POST_DIFF_FRAMES
        const step = () => {
          if (framesLeft-- > 0) {
            requestAnimationFrame(step)
            return
          }
          const modEd = editor.getModifiedEditor()
          const origEd = editor.getOriginalEditor()
          const h = Math.max(modEd.getContentHeight(), origEd.getContentHeight())
          finish(h > 0 ? h : null)
        }
        requestAnimationFrame(step)
      })

      const timer = window.setTimeout(() => finish(null), MEASURE_TIMEOUT_MS)

      // Bind AFTER wiring up the listener so we don't miss the event.
      editor.setModel({ original, modified })
    })
  }
}

// Module-level singleton — one preloader per app, matching the pool.
let instance: DiffHeightPreloader | null = null

export function getDiffHeightPreloader(): DiffHeightPreloader {
  if (!instance) instance = new DiffHeightPreloader()
  return instance
}

export function disposeDiffHeightPreloader(): void {
  instance?.dispose()
  instance = null
}

export { DEFAULT_HEIGHT as DEFAULT_DIFF_ROW_HEIGHT }
