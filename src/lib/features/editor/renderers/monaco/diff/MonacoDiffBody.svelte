<!--
  @component
  Monaco-backed diff body for a single file. Gates on viewport, hands
  off to the pooled DiffEditor, roundtrips view-state on release, and
  forwards content height to the outer virtual scroll. Does NOT own
  file data (DiffModelStore), editor config (pool), or the file list
  (DiffStack); it only retains live Monaco models while mounted.
-->

<script lang="ts">
  import DiffBinaryPlaceholder from '$lib/features/workbench/surfaces/diff/DiffBinaryPlaceholder.svelte'
  import { getDiffEditorPool, type PoolRef } from '$lib/features/editor/renderers/monaco/diff/editorPool.svelte'
  import { useDiffScroll } from '$lib/features/editor/renderers/monaco/diff/scrollContext.svelte'
  import {
    registerSwormDiffGutterContext,
    SWORM_DIFF_GIT_ACTIONS_CONTEXT,
    SWORM_DIFF_STAGED_CONTEXT
  } from '$lib/features/editor/renderers/monaco/diff/gitGutterMenu'
  import type { DiffModelStore } from '$lib/features/workbench/surfaces/diff/diffModels.svelte'
  import {
    runDiffGitLineAction,
    titleForDiffGitLineAction,
    type DiffGitLineAction
  } from '$lib/features/workbench/surfaces/diff/diffGitLineActions'
  import {
    lineChangeIntersectsRanges,
    lineChangesOutsideRanges,
    selectedLineChanges,
    toLineSelectionRanges,
    type LineChange,
    type LineSelectionRange
  } from '$lib/features/git/lineChanges'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import type { GitStatusKind } from '$lib/types/backend'

  type IDiffEditorViewState = import('monaco-editor').editor.IDiffEditorViewState
  type ICodeEditor = import('monaco-editor').editor.ICodeEditor
  type ILineChange = import('monaco-editor').editor.ILineChange
  type IDisposable = { dispose(): void }
  type ContextKey<T> = { set(value: T): void; reset(): void }
  type DiffEditorWithRootContext = PoolRef['editor'] & {
    _contextKeyService?: {
      createKey<T>(key: string, defaultValue: T): ContextKey<T>
    }
  }

  interface GitActionContext {
    projectId: string
    projectPath: string
    staged: boolean
    status: GitStatusKind
  }

  let {
    path,
    store,
    hideUnchanged = true,
    hideUnchangedCommandSeq = 0,
    gitActionContext = null,
    onExpandedUnchangedChange
  }: {
    path: string
    store: DiffModelStore
    /**
     * Drives Monaco's per-editor `hideUnchangedRegions`. Flipping this
     * while the editor is bound applies immediately via updateOptions;
     * for unbound (parked) editors, the value is picked up on the next
     * acquire via `entry.hideUnchanged`.
     */
    hideUnchanged?: boolean
    hideUnchangedCommandSeq?: number
    gitActionContext?: GitActionContext | null
    onExpandedUnchangedChange?: (expanded: boolean) => void
  } = $props()

  const pool = getDiffEditorPool()
  const scroll = useDiffScroll()

  let host = $state<HTMLDivElement | null>(null)
  // Seed placeholder height. Matches VSCode's MultiDiffEditor default
  // (`lastTemplateData.contentHeight = 500`) — closer to typical diff
  // row heights than the old 240 value, so the one-shot seed → real
  // transition is smaller on first-view files the preloader hasn't
  // finished yet. Overridden from `entry.height` (preloader-populated
  // or cached from a prior mount) once the store is available.
  let height = $state(500)
  let visible = $state(false)

  // Ownership state — purely imperative, not reactive.
  let currentRef: PoolRef | null = null
  let currentDisposables: Array<{ dispose(): void }> = []
  let currentAcquireSeq = 0
  let currentMeasureAndSync: (() => void) | null = null
  let currentLineChanges: LineChange[] = []

  // Monaco serializes collapsed unchanged-region state into
  // `viewState.modelState`. That fights the row-level toggle, whose
  // semantics are binary: either all unchanged code is collapsed or
  // all of it is shown. Keep scroll/selections, but let the row toggle
  // own hidden-region state.
  function stripModelState(state: IDiffEditorViewState | null): IDiffEditorViewState | null {
    if (!state) return null
    return {
      original: state.original,
      modified: state.modified
    }
  }

  function syncExpandedUnchangedState(ref: PoolRef, hide: boolean) {
    onExpandedUnchangedChange?.(pool.hasExpandedUnchanged(ref, hide))
  }

  function bindDiffGutterContextKeys(ref: PoolRef, context: GitActionContext | null): IDisposable {
    const rootContextService = (ref.editor as DiffEditorWithRootContext)._contextKeyService
    if (!rootContextService) return { dispose: () => {} }

    // Monaco's public IStandaloneDiffEditor.createContextKey() binds to the
    // modified editor. DiffEditorGutter reads the diff widget root context, so
    // these toolbar predicates must be created on the internal root service.
    const hasGitActionsContext = rootContextService.createKey<boolean>(SWORM_DIFF_GIT_ACTIONS_CONTEXT, false)
    const stagedContext = rootContextService.createKey<boolean>(SWORM_DIFF_STAGED_CONTEXT, false)
    hasGitActionsContext.set(Boolean(context))
    stagedContext.set(Boolean(context?.staged))

    return {
      dispose: () => {
        hasGitActionsContext.reset()
        stagedContext.reset()
      }
    }
  }

  function toLineChange(change: ILineChange): LineChange {
    return {
      originalStartLineNumber: change.originalStartLineNumber,
      originalEndLineNumber: change.originalEndLineNumber,
      modifiedStartLineNumber: change.modifiedStartLineNumber,
      modifiedEndLineNumber: change.modifiedEndLineNumber
    }
  }

  function selectionRanges(editor: ICodeEditor): LineSelectionRange[] {
    return toLineSelectionRanges(editor.getSelections() ?? [])
  }

  function hasSelectedChanges(ranges: readonly LineSelectionRange[], modifiedLineCount: number): boolean {
    return currentLineChanges.some((change) => lineChangeIntersectsRanges(change, ranges, modifiedLineCount))
  }

  async function runLineAction(action: DiffGitLineAction, changes: readonly LineChange[]) {
    const context = gitActionContext
    const entry = store.get(path)
    if (!context || !entry) return
    if (changes.length === 0 && action !== 'revert') {
      notify.info('No changes selected', 'The selected range does not contain a diff hunk.')
      return
    }

    try {
      await runDiffGitLineAction(
        {
          projectId: context.projectId,
          projectPath: context.projectPath,
          filePath: path,
          status: context.status
        },
        entry,
        action,
        changes
      )
    } catch (error) {
      notify.error(`${titleForDiffGitLineAction(action)} failed`, getErrorMessage(error))
    }
  }

  async function runSelectedAction(action: DiffGitLineAction) {
    const ref = currentRef
    if (!ref) return

    const modifiedEditor = ref.editor.getModifiedEditor()
    const model = modifiedEditor.getModel()
    const modifiedLineCount = model?.getLineCount() ?? 1
    const ranges = selectionRanges(modifiedEditor)
    const entry = store.get(path)
    const content = entry
      ? {
          originalContent: entry.originalContent,
          modifiedContent: entry.modifiedContent
        }
      : undefined

    if (action === 'revert') {
      if (!hasSelectedChanges(ranges, modifiedLineCount)) {
        notify.info('No changes selected', 'The selected range does not contain a diff hunk.')
        return
      }
      const remaining = lineChangesOutsideRanges(currentLineChanges, ranges, modifiedLineCount, content)
      await runLineAction(action, remaining)
      return
    }

    const selected = selectedLineChanges(currentLineChanges, ranges, modifiedLineCount, content)
    await runLineAction(action, selected)
  }

  // Flipping the hideUnchanged prop OR clicking the row action while
  // Monaco's live state has drifted from the persisted preference
  // should both re-issue the command. The explicit sequence number
  // covers the "some regions manually opened but hideUnchanged is still
  // true" case, where the boolean alone would be a no-op.
  $effect(() => {
    const hide = hideUnchanged
    const commandSeq = hideUnchangedCommandSeq
    const ref = currentRef
    if (!ref) return

    void commandSeq
    pool.applyHideUnchanged(ref, hide)
    syncExpandedUnchangedState(ref, hide)
    requestAnimationFrame(() => currentMeasureAndSync?.())
  })

  $effect(() => {
    if (!host) return
    // Distinguish "rendered standalone" (no context at all) from
    // "inside DiffStack but scroll element not mounted yet". The
    // former forces-visible so the component is usable outside the
    // stack; the latter waits so we don't burst-mount every file's
    // editor on first render.
    if (!scroll) {
      visible = true
      return
    }
    const rootEl = scroll.element
    if (!rootEl) {
      visible = false
      return
    }
    const io = new IntersectionObserver(
      ([entry]) => {
        visible = entry.isIntersecting
      },
      { root: rootEl, rootMargin: '400px 0px' }
    )
    io.observe(host)
    return () => io.disconnect()
  })

  $effect(() => {
    const entry = store.get(path)
    if (!visible || !host || !entry || entry.binary) return
    const retainedEntry = store.retain(path)
    if (!retainedEntry || retainedEntry.binary) return

    const original = retainedEntry.original
    const modified = retainedEntry.modified
    if (!original || !modified) {
      store.release(path)
      return
    }

    // Restore last-known height before the first content-size event
    // arrives so collapsed→expanded transitions don't jump.
    if (retainedEntry.height != null) height = retainedEntry.height

    const hostEl = host
    const seq = ++currentAcquireSeq

    void (async () => {
      const { ref, stickyReuse } = await pool.acquire(hostEl, path)
      if (seq !== currentAcquireSeq) {
        // Visibility flipped while we awaited — release the acquired
        // editor rather than binding models we'll only tear down again.
        pool.release(ref)
        return
      }

      // Clear any stale view zones from the previous model before
      // binding the new one. Swapping in a single call can leave
      // zones pointing at line numbers that don't exist in the new
      // model — which surfaces as "Illegal value for lineNumber"
      // from Monaco's hideUnchangedRegions machinery.
      ref.editor.setModel(null)
      // Seed the option before binding the model so Monaco constructs
      // the diff with the correct unchanged-region feature from the
      // first paint.
      pool.seedHideUnchanged(ref, hideUnchanged)
      ref.editor.setModel({ original, modified })

      if (stickyReuse && retainedEntry.viewState) {
        try {
          ref.editor.restoreViewState(stripModelState(retainedEntry.viewState))
        } catch {
          // Malformed cached state — silent reset.
        }
      }

      // Re-apply the row-level unchanged-region state after binding the
      // model and restoring scroll/selections. This keeps live toggle
      // semantics deterministic on fresh acquire and sticky reuse.
      pool.applyHideUnchanged(ref, hideUnchanged)
      syncExpandedUnchangedState(ref, hideUnchanged)

      // Row height tracks whichever inner pane is tallest. In split
      // view the diff editor coordinates both sides to the same
      // display height, so max(modifiedContentHeight, originalContentHeight)
      // is the row's true content height.
      //
      // Measurement is gated on the first `onDidUpdateDiff` event.
      // Until Monaco's worker has computed the diff, `getContentHeight`
      // returns the full modified file — committing that would grow
      // the host to full-file size, force Monaco to paint every line,
      // then shrink back when `hideUnchangedRegions` collapses the
      // unchanged body. Gating on diff-ready guarantees the first
      // committed measurement is the real diff height.
      const modifiedEd = ref.editor.getModifiedEditor()
      const originalEd = ref.editor.getOriginalEditor()

      let diffReady = false

      const updateLineChanges = () => {
        currentLineChanges = (ref.editor.getLineChanges() ?? []).map(toLineChange)
      }

      const measureAndSync = () => {
        if (!diffReady) return
        const h = Math.max(modifiedEd.getContentHeight(), originalEd.getContentHeight())
        if (h <= 0) return

        if (h !== height) {
          height = h
          store.setHeight(path, h)
        }

        // The measured row height is the source of truth. Apply that
        // same number directly to Monaco's viewport instead of waiting
        // for a later host resize observer cycle, which can miss the
        // initial 240px -> real-height jump and leave Monaco stuck with
        // an internal hidden Y scroll.
        ref.editor.layout({ width: hostEl.clientWidth, height: h })
      }
      currentMeasureAndSync = measureAndSync

      // Safety net: if `onDidUpdateDiff` never fires (malformed models,
      // empty models, Monaco worker wedge) flip the gate after 800ms so
      // the row isn't stuck at the seed height forever. Normal paths
      // beat this easily — the worker typically settles within a frame
      // or two.
      const safetyTimer = window.setTimeout(() => {
        if (diffReady) return
        diffReady = true
        updateLineChanges()
        measureAndSync()
      }, 800)
      const safetyOff = { dispose: () => window.clearTimeout(safetyTimer) }

      const sizeOffMod = modifiedEd.onDidContentSizeChange(measureAndSync)
      const sizeOffOrig = originalEd.onDidContentSizeChange(measureAndSync)
      // `onDidUpdateDiff` fires when Monaco's diff worker completes —
      // but the `hideUnchangedRegions` widgets materialize on a later
      // paint, so reading `getContentHeight()` at this exact moment
      // still returns the uncollapsed full-file height. Waiting two
      // animation frames gives the layout pipeline time to apply the
      // widgets, so the FIRST committed measurement is the real
      // post-collapse height rather than a brief overshoot.
      const diffOff = ref.editor.onDidUpdateDiff(() => {
        updateLineChanges()
        if (diffReady) {
          measureAndSync()
          return
        }
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            if (diffReady) return
            diffReady = true
            measureAndSync()
          })
        })
      })
      // `hideUnchangedRegions` and folding change visible line count
      // without necessarily firing `onDidContentSizeChange`. This is
      // the dedicated event for that transition — without it, expanding
      // a collapsed region adds visible lines but the row doesn't grow.
      const syncHiddenAreas = () => {
        measureAndSync()
        syncExpandedUnchangedState(ref, hideUnchanged)
      }
      const hiddenOffMod = modifiedEd.onDidChangeHiddenAreas(syncHiddenAreas)
      const hiddenOffOrig = originalEd.onDidChangeHiddenAreas(syncHiddenAreas)
      const actionDisposables: IDisposable[] = []
      if (gitActionContext) {
        if (gitActionContext.staged) {
          actionDisposables.push(
            modifiedEd.addAction({
              id: 'sworm.diff.unstageSelectedRanges',
              label: 'Unstage Selected Ranges',
              contextMenuGroupId: '2_git',
              contextMenuOrder: 1,
              run: () => runSelectedAction('unstage')
            })
          )
        } else {
          actionDisposables.push(
            modifiedEd.addAction({
              id: 'sworm.diff.stageSelectedRanges',
              label: 'Stage Selected Ranges',
              contextMenuGroupId: '2_git',
              contextMenuOrder: 1,
              run: () => runSelectedAction('stage')
            }),
            modifiedEd.addAction({
              id: 'sworm.diff.revertSelectedRanges',
              label: 'Revert Selected Ranges',
              contextMenuGroupId: '2_git',
              contextMenuOrder: 2,
              run: () => runSelectedAction('revert')
            })
          )
        }
      }

      const contextKeyOff = bindDiffGutterContextKeys(ref, gitActionContext)

      const gutterContextOff =
        gitActionContext && retainedEntry.modified
          ? registerSwormDiffGutterContext(retainedEntry.modified.uri.toString(), {
              projectId: gitActionContext.projectId,
              projectPath: gitActionContext.projectPath,
              filePath: path,
              status: gitActionContext.status,
              staged: gitActionContext.staged,
              store,
              getLineChanges: () => currentLineChanges
            })
          : null

      // Re-layout Monaco on ANY host size change. With
      // `automaticLayout: false`, Monaco doesn't observe its own
      // container — if we don't call layout() when the row grows
      // vertically, Monaco's internal viewport stays at the stale
      // size and everything past that is clipped.
      //
      // Deferred to the next frame via RAF so that Monaco's layout()
      // runs AFTER the observer callback returns — if layout() ran
      // synchronously it would re-dirty the sizes this same observer
      // is watching, which browsers report as
      // "ResizeObserver loop completed with undelivered notifications".
      let roPending = false
      const ro = new ResizeObserver(() => {
        if (roPending) return
        roPending = true
        requestAnimationFrame(() => {
          roPending = false
          ref.editor.layout({ width: hostEl.clientWidth, height: hostEl.clientHeight })
        })
      })
      ro.observe(hostEl)
      const roOff = { dispose: () => ro.disconnect() }

      currentRef = ref
      updateLineChanges()
      currentDisposables = [
        sizeOffMod,
        sizeOffOrig,
        diffOff,
        hiddenOffMod,
        hiddenOffOrig,
        ...actionDisposables,
        contextKeyOff,
        ...(gutterContextOff ? [gutterContextOff] : []),
        roOff,
        safetyOff
      ]
    })()

    return () => {
      currentAcquireSeq++
      if (currentRef) {
        // Dispose listeners BEFORE release so the pool's `setModel(null)`
        // doesn't fire a size event into a stale callback.
        for (const d of currentDisposables) d.dispose()
        try {
          const state = currentRef.editor.saveViewState()
          if (state) store.setViewState(path, stripModelState(state))
        } catch {
          // saveViewState can throw mid-reparent; best-effort only.
        }
        pool.release(currentRef)
        currentRef = null
        currentDisposables = []
        currentMeasureAndSync = null
        currentLineChanges = []
      }
      store.release(path)
    }
  })
</script>

{#if store.get(path)?.binary}
  <DiffBinaryPlaceholder reason="Binary file — content not shown" />
{:else}
  <div bind:this={host} class="relative w-full" style:height="{height}px"></div>
{/if}
