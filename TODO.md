# TODO

## Ongoing

- [ ] Keep CommandCenter updated
  Register new features, views, and actions as commands so they are discoverable in the command palette and keyboard shortcuts.

## Backlog

### UI

- [ ] `VariableHeightVirtualList` sibling primitive
  Land a measurement-based variant of `FixedHeightVirtualList` that caches per-row heights (cached on `entry.height` for diff rows), then migrate `DiffStack.svelte` onto it. Today the diff stack ships its own `setDiffScrollContext` + `IntersectionObserver` (in `MonacoDiffBody.svelte`) + `heightPreloader` pipeline while the file tree uses `FixedHeightVirtualList`. One shared primitive set keeps both stacks coherent.
  `DiffStack.svelte` mounts a `DiffStackFile` header for every file (measured: 1,000 files ≈ 305 ms mount, ~12k DOM nodes); window headers as part of this migration.
- [ ] Overscroll feature
  Allow elastic overscroll when scrolling past the end of a scrollable surface.
  Start with a single-surface prototype and settle the threshold, max distance, trigger input, and return behavior before expanding it.

### Git Stash UX

- [ ] Stash operation error handling
  Pop/apply conflicts already route to the Changes pane, but the guidance says "then continue" (stash has no operation to continue) and never says the entry was kept and must be dropped after resolving. "Local changes would be overwritten" and drop failures fall through to a generic toast. `popStash` reuses the "Applying stash" / "Stash applied" notification text.
- [ ] Stash action button states
  Add in-flight or disabled states for the row Pop / Drop buttons and the context-menu Apply / Pop / Branch items so rapid clicks cannot race shifted stash indices (two quick Pops on `stash@{0}` pop two entries).

### Editor

- [ ] Editor settings
  There is no `editor` settings section; only code font family/size are configurable (via `typography`). `renderWhitespace: 'all'`, `tabSize: 2`, and `insertSpaces` are hardcoded in `MonacoEditor.svelte`; `wordWrap` is markdown-only; indent rainbow is an in-memory toggle that resets on reload.

### Language Support

- [ ] Bash: bash-language-server builtin, shfmt formatter
- [ ] Java: jdtls builtin
- [ ] Markdown: marksman builtin
- [ ] TOML: real highlighting (currently mapped to `ini`; add a Shiki grammar) and a taplo builtin
- [ ] YAML: yaml-language-server builtin
- [ ] JSX / TSX: `.jsx`/`.tsx` map to plain `javascript`/`typescript` with no `javascriptreact`/`typescriptreact` language IDs; verify LSP servers handle JSX correctly

### Diff View

- [ ] Re-focus already-open diff tabs correctly
  `addContentTab` replaces a temporary diff tab before checking for a matching pinned one, so an open preview diff causes a duplicate "Changes" tab instead of refocusing it. Commit and stash diff tabs have no reveal nonce, so clicking the same file again does not re-scroll.
- [ ] Investigate editable diff mode
- [ ] Add a visual focus cue when opening a diff-stack file from the git tree

### Markdown

- [ ] Notes sidebar
  Add a notes activity bar view that pins common markdown files and lists the rest of the folder's `.md` / `.mdx` files.
- [ ] Interactive checklists
  `remark-gfm` already renders `- [ ]` / `- [x]` as disabled checkboxes; make them clickable and write the toggle back to the source.
- [ ] Create new note
  Add a button in the notes sidebar to create a new `.md` file.

### Workbench Restore

- [ ] Render the workbench before restore finishes
  `WorkbenchPage.svelte` shows "Restoring workspace..." until the whole `restoreWorkbench` → `initGroupService` → `window.ready` chain resolves, and `restoreWorkbench` runs its steps serially.
- [ ] Reload View: warn about running terminals and tasks
  Reload already confirms dirty editors; it does not check live PTYs or tasks.

### File Handling & File Explorer

- [ ] Vykar repo backups (beta opt-in)
  Add an opt-in backup management section to the files sidebar.
- [ ] Inline rename for files and folders
  Replace the `PromptDialog` (also used for New File / New Folder) with an in-tree input.
- [ ] Paste: safety cap, progress, and cancel
  `FileService::paste` has no size/count limit, progress, or cancel, and the UI shows only a final success toast. The frontend already pastes one source per call, a natural place to hook progress and cancel.
- [ ] Symlink support in file operations
  Copy (and cross-filesystem cut) follows symlinks instead of preserving them and fails on links to directories. `delete` and `paste_collisions` use `exists()`, so broken symlinks cannot be deleted or detected as collisions.
- [ ] File operations follow-through
  Add a Duplicate action and an explicit Delete Permanently (menu item / Shift+Delete). The delete dialog says "Delete File" for folders too.
- [ ] File preview/viewer for common types
  Images, audio, video, and markdown preview exist. Missing: JSON/CSV/structured data views, SVG as image, PDF, and media preview for files opened at a git ref.

### Notifications & Feedback

- [ ] Notification update API should allow clearing descriptions
  `NotificationUpdate.description` ignores `undefined`, so `runNotifiedTask` success toasts keep the loading description.
- [ ] Progress + cancel in notifications
  `Notification` has no progress or cancel fields and there are no shared cancel semantics; the only cancel path (large-file reads) lives inside `FileEditor`, outside the store.

### Terminal / PTY

- [ ] WebGL renderer for task terminals
  Session terminals load `@xterm/addon-webgl`; task terminals (`tasks/terminal.ts`) still use the DOM renderer.

### Performance

- [ ] Switch desktop PTY `Channel<Vec<u8>>` to `tauri::ipc::InvokeResponseBody::Raw`
  Desktop session/task/window channels still send JSON number arrays (~3.5× binary size) decoded via `new Uint8Array(bytes)` in `transport.desktop.ts`. Web/remote already use raw frames.
- [ ] Revisit git reconcile polling
  `git_watcher` now pushes `GitChanged` events and invalidates the 300 ms summary cache, yet the active folder still polls every 2 s (`RECONCILE_INTERVAL_MS`). Decide whether the poll can be slowed or dropped.
