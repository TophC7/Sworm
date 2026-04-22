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
  import type { DiffModelStore } from '$lib/features/workbench/surfaces/diff/diffModels.svelte'

  type IDiffEditorViewState = import('monaco-editor').editor.IDiffEditorViewState

  let {
    path,
    store,
    hideUnchanged = true,
    hideUnchangedCommandSeq = 0,
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
      currentDisposables = [sizeOffMod, sizeOffOrig, diffOff, hiddenOffMod, hiddenOffOrig, roOff, safetyOff]
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
      }
      store.release(path)
    }
  })
</script>

{#if store.get(path)?.binary}
  <DiffBinaryPlaceholder reason="Binary file — content not shown" />
{:else}
  <div bind:this={host} class="w-full" style:height="{height}px"></div>
{/if}
