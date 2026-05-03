# Stable Git Diff Rendering

**Started**: 2026-05-03
**Shape**: light
**Beads label**: `spec:stable-git-diff-rendering`

## §G — Goal

The git diff viewer should feel physically stable while the user scrolls. After a diff view has loaded its current git data and the user has chosen any view toggles, ordinary scrolling must not visibly change rendered content, row heights, header positions, expanded state, or file identity. The work should keep first paint fast where possible, but it may do extra precomputation or background hydration when that is the best way to eliminate scroll-driven shifting. This applies to every git diff entry point because `WorkingDiffView.svelte`, `CommitDiffView.svelte`, and `StashDiffView.svelte` all converge on the shared `DiffStack` / `DiffStackFile` / `MonacoDiffBody` rendering path.

## §C — Constraints (locked decisions + out-of-scope)

- **Shape**: This is a light spec with local beads tracking. The work is cohesive enough to keep in one document, but the execution tasks need durable queue state.
- **Scope**: Fix all git diff views that use the shared diff stack: working unstaged, working staged, commit, and stash. Even if the root cause is in one shared component, verification must cover every entry point because data loading differs across the views.
- **User-visible contract**: Scroll may attach or detach internal Monaco resources if that is necessary for the virtualized design, but it must not create visible layout shifts, row header movement, changed file ordering, changed expanded state, or changed rendered code for already-loaded data.
- **Performance balance**: Preserve fast first paint when it does not conflict with stability. If eliminating shifts requires extra precompute, background hydration, or broader renderer changes, investigate those options and choose the best balance instead of forcing the current approach to remain unchanged.
- **Expand-all behavior**: `Expand all` should respond immediately. It should not make the UI feel stuck waiting for every file's Monaco diff to finish. If all heights are not known yet, the implementation should reserve deterministic placeholder heights or run targeted preloading so visible rows do not jump later. Blocking the command until every height is measured is allowed only if the investigation proves it is the best option.
- **Out of scope discipline**: There is no hard ban on renderer rewrites, staging and revert plumbing changes, backend metadata changes, or UI control changes if they are required for the best stable solution. Unrelated styling redesigns, unrelated git semantics, and unrelated editor work remain out of scope.
- **Root-cause discipline**: Do not ship a speculative patch. The first task must prove which lifecycle events cause the shifting before implementation begins.
- **Verification style**: Prefer manual UX verification over a synthetic automated visual test. Use `bun run check` for baseline type and Svelte validation.

## §I — Interfaces (touched files / public surfaces)

- `src/lib/features/workbench/surfaces/diff/DiffStack.svelte` — Owns the file list, expanded state, scroll container, global diff settings, height preloading, and the scroll context currently updated during scroll.
- `src/lib/features/workbench/surfaces/diff/DiffStackFile.svelte` — Owns row headers, per-file controls, expanded body mounting, and per-file unchanged-code preference.
- `src/lib/features/editor/renderers/monaco/diff/MonacoDiffBody.svelte` — Owns viewport gating, editor acquisition and release, height seeding, height measurement, hide-unchanged synchronization, and Monaco layout calls.
- `src/lib/features/editor/renderers/monaco/diff/scrollContext.svelte.ts` — Defines the shared scroll state. This is a likely interface to change if scroll position must stop invalidating reactive consumers.
- `src/lib/features/editor/renderers/monaco/diff/editorPool.svelte.ts` — Owns pooled Monaco diff editors and global settings propagation. Changes here must preserve model ownership boundaries.
- `src/lib/features/editor/renderers/monaco/diff/heightPreloader.svelte.ts` — Owns off-screen height precomputation. This may become more central, be retuned, or be replaced if another approach is more stable.
- `src/lib/features/workbench/surfaces/diff/diffModels.svelte.ts` — Owns diff model entries, cached height, cached view state, lazy content fetches, and content LRU behavior.
- `src/lib/features/workbench/surfaces/diff/WorkingDiffView.svelte` — Uses lazy working-tree index and per-file content fetches. It needs coverage because data can arrive while scrolling.
- `src/lib/features/workbench/surfaces/diff/CommitDiffView.svelte` — Uses eager commit payloads. It needs coverage to ensure the shared fix works without the lazy working-tree path.
- `src/lib/features/workbench/surfaces/diff/StashDiffView.svelte` — Uses eager stash payloads. It needs coverage to ensure stash data shape does not expose a separate height or status edge case.
- `src-tauri/src/models/file_diff.rs` and `src-tauri/src/services/git.rs` — Only change these if the chosen frontend fix needs stable height metadata, hunk metadata, or content signatures that cannot be derived cheaply in the frontend.

## §V — Invariants (cross-cutting rules)

- Ordinary scrolling must not be treated as a semantic render trigger. Verify by instrumenting scroll on a large expanded diff and confirming that scroll position changes do not rerender stable row headers or recompute stable file props.
- A row's user-visible height may change only when git data changes, when the row is explicitly expanded or collapsed, when the user changes a view setting such as wrap, side-by-side, font size, or hide-unchanged, or when a currently visible placeholder resolves in a controlled way that does not move already-inspected content.
- File identity must remain keyed by `file.path` or a deliberate replacement key if rename/copy collisions require one. Verify that scrolling does not remount row chrome for unchanged `files`.
- Lazy working-tree content fetches must not cause viewport-wide layout churn. A fetched file may hydrate its own body, but it must not move unrelated rows in a way the user can see.
- Monaco editor acquisition and release may happen for virtualization, but it must be invisible. Verify that acquire/release cycles do not change saved view state, hide-unchanged preference, header position, or row height after the row has stabilized.
- Height measurement must be monotonic from the user's perspective within one data/toggle state. If the implementation uses estimates, the estimate must reserve enough space or swap to measured content in a way that avoids visible upward and downward jumps during normal scroll.
- Manual verification is required for working unstaged, working staged, commit, and stash views before closing the spec. Verify: `bun run check` succeeds after implementation.

## Current State

<!-- spec-sync:start -->
**Last synced**: 2026-05-03
**Progress**: 0 closed, 0 in progress, 5 open
**Ready next**: `ADE-6q5` · P1 task · Instrument git diff render and height churn
**Blockers**: none
**Follow-ups**: none
**Backprop bugs**: none
<!-- spec-sync:end -->

## §T — Tasks

| bd-id | spec-task | deps | summary | acceptance |
|-------|-----------|------|---------|------------|
| `ADE-6q5` | T.1 | — | Instrument `DiffStack`, `MonacoDiffBody`, `DiffModelStore`, `DiffHeightPreloader`, and `DiffEditorPool` to prove which scroll-time events cause rerenders, remounts, content loads, or height changes. | Manual repro identifies the exact scroll-triggered render, mount, release, fetch, or height mutation path for working, commit, and stash diff views. Temporary diagnostics are removed before closing. |
| `ADE-a3l` | T.2 | T.1 | Define the stable viewport contract in implementation terms: row identity, row height ownership, editor acquire/release rules, lazy content rules, and setting-change rules. | The chosen approach is grounded in T.1 evidence and documented in code-adjacent notes or comments where the invariant would otherwise be easy to break. |
| `ADE-xc7` | T.3 | T.2 | Stabilize row height lifecycle so expanded diff rows reserve deterministic space before scroll reaches them and scroll-only measurement cycles cannot mutate visible layout. | With unchanged git data and unchanged user toggles, ordinary scrolling through expanded rows does not visibly change row heights, move headers, or churn Monaco bodies in a way that affects the visible viewport. |
| `ADE-fmv` | T.4 | T.2 | Keep scroll out of reactive render state. Replace scroll-driven rune invalidation with imperative viewport observation or a virtualizer-safe signal that cannot rerender stable row chrome on every scroll frame. | Manual scroll repro shows `scrollTop` changes do not rerender stable row chrome or recompute file props. Only viewport attach/detach internals run, and those internals do not move visible content. |
| `ADE-swo` | T.5 | T.3, T.4 | Verify all diff sources and controls manually: working unstaged, working staged, commit, stash, many-file diffs, small files, large files, binary or oversized rows, renames, expand all, collapse all, wrap, side-by-side, font size, and per-file hide-unchanged. | Manual verification notes record pass or remaining issue for every listed view and toggle. `bun run check` succeeds. |

## §B — Bugs (discovered during execution)

| id | severity | discovery | fix-target |
|----|----------|-----------|------------|

## Acceptance

- [ ] Every §T bead is closed.
- [ ] `bun run check` succeeds.
- [ ] Manual verification covers working unstaged, working staged, commit, and stash diff views.
- [ ] Manual verification confirms scrolling alone does not change visible layout or rendered content after data and toggles are stable.
- [ ] Temporary instrumentation is removed.
- [ ] §B is empty or every row is resolved.

## Risks

- **False stability from narrow repro**: A fix can look stable on one diff but still shift in lazy working-tree data or stash/commit eager data. Verify every git diff entry point, not only the view used during development.
- **Hidden Monaco lifecycle coupling**: Monaco diff internals expose useful state only through a mix of public and private APIs. Changes to pooling, hide-unchanged regions, or view-state restore can reintroduce subtle layout churn. Keep changes small until the root cause is proven.
- **Over-preloading**: Precomputing every row can eliminate shifts but harm first paint or memory on very large diffs. Measure the tradeoff before choosing eager work.
- **Under-reserved placeholder height**: If placeholder heights are too small, rows will still grow during scroll. If they are too large, the list can feel sparse or jump when collapsing. The chosen strategy must be tested on both small and large files.
- **Reactive scroll feedback loop**: Any `$state` or `$derived` path that changes on every scroll frame can invalidate more of the tree than intended. The implementation must make scroll observation local and imperative unless Svelte reactivity is proven harmless.

## Notes

- Current suspicious paths from code reading are `DiffStack.svelte:121`, where height preloading skips lazy entries without content; `DiffStack.svelte:220`, where scroll context mutates reactive state during scroll; `MonacoDiffBody.svelte:263`, where `IntersectionObserver` toggles visibility; and `MonacoDiffBody.svelte:356`, where measured height writes to Svelte state and the model store.
- The working-tree view has a unique risk because `WorkingDiffView.svelte:46` reloads a cheap index and loads file content lazily through `DiffModelStore`. Commit and stash views still need coverage because they share the same renderer and may have different row-height behavior with eager content.
- If this light spec grows beyond these tasks, convert it to a phased spec rather than continuing to add unrelated renderer work here.
