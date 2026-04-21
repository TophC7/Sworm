# TODO

## 🔄 In Progress / Ongoing

### UI

- [ ] **Ongoing: keep CommandCenter updated** — whenever new features, views, or actions are added to the app, register them as commands in CommandCenter so they're discoverable via the command palette and keyboard shortcuts. This includes file operations, git actions, workspace controls, etc.

### File Explorer

- [x] **Single-file diff view** — Git file context menus now open Monaco-backed Changes tabs for a single file (working copy vs HEAD)
- [ ] **WIP: Per-file stash** — "Stash Changes" in the git context menu should stash individual files (`git stash push -- <file>`). Currently disabled in GitContextMenu
- [x] **Folder-scoped diff view** — Git folder context menus now open scoped Changes tabs for files under that directory

---

## 📋 Backlog

### UI

- [ ] Recursive split-tree panes with size + count caps — replace the fixed `SplitMode`/`QuadLayout` state machine in `workspace.svelte.ts` with a recursive `Node = Leaf | Branch` tree (direction + sizes + children[]) so arbitrary layouts like 2-top+1-bottom or 3-column become reachable. Gate new splits with two caps: (A) measured min pane size — register each pane's element in a `WeakMap<PaneId, HTMLElement>` from `Pane.svelte` on attach, and have `canSplitPane(leafId, direction)` reject splits that would produce panes smaller than `MIN_PANE_PX` (~240px) via `getBoundingClientRect()`; (B) hard `MAX_PANES` count cap (~8) as a safety net. Drop overlay should dim + relabel ("Pane too small" / "Max panes reached") when rejected. Needs workspace persistence v1→v2 migration for the tree schema and updates in `PaneGrid.svelte`, `Pane.svelte`, and the pane drop observer.
- [ ] Pane overscroll feature — when scrolling past the end of content in a pane, allow elastic overscroll that pushes content up so the end feels reachable. Should have configurable threshold, max overscroll amount, and spring animation on return. Blocked on: threshold definition (immediate vs amount), max distance cap, trigger input (wheel only vs keyboard/trackpad), return behavior (scroll up vs any scroll down), handling of edge cases (empty/short content, text selection at bottom, window resize while overscrolled, keyboard nav during overscroll, momentum scrolling). Start with single pane prototype, not all panes.

### Keyboard Shortcuts

- [ ] Allow binding a shortcut to commands that don't have one by default — Command Center's pencil icon currently only renders for rows that already have a default shortcut (`{#if effectiveShortcut || cmd.shortcut}` gate in CommandCenter.svelte). Widen so any rebindable command surfaces the pencil, letting users assign a shortcut to e.g. Toggle Sidebar or any future commandless action.
- [ ] Palette row for `toggle-command-palette` itself — it's registered as a rebindable id in shortcuts.svelte.ts but has no Command Center entry, so the rebind UI can't reach it. Either add a palette row (recursive but harmless — opening the palette to toggle the palette) or surface the palette's own shortcut in a settings view.

### Git Stash UX

- [ ] Stash operation error handling — `stash pop` can fail with merge conflicts leaving the working tree in a conflicted state, `stash drop` can also fail. Currently errors are silently caught and logged to console. Needs proper conflict detection, user-visible error states, and guided recovery (not just a toast — the UI should reflect the conflicted state and offer resolution paths)
- [ ] Stash action button states — Pop/Drop/Apply buttons have no in-flight/disabled state during async operations. Rapid clicks can operate on shifted indices after a pop. Needs operation locking, optimistic UI updates, or at minimum disabling buttons while an operation is pending. Ties into the error handling above since both need a coherent "operation in progress" model

### Editor

- [ ] Editor settings — expose Monaco options and editor extensions as user-configurable settings (similar to VS Code's `.vscode/settings.json`). Start with: `renderWhitespace` (currently hardcoded to `'all'`), indent rainbow toggle (currently command-only), `tabSize`, `fontSize`, `wordWrap`. Store per-project or globally. Eventually support a `.sworm/settings.json` or equivalent
- [ ] Bundled LSP runtime support — add real launch support for `bundled_binary` / `bundled_js` extension manifests by packaging server assets into app/extension resources, resolving those resource paths at runtime, and surfacing clear status/errors in `Settings -> Languages`
- [ ] Open in Fresh from editor toolbar — the "Open in Fresh" icon button in FileEditor's toolbar doesn't work when no Fresh session is pre-existing. `ensureFreshSession` creates the session and `waitForSessionReady` confirms the PTY is running, but `backend.editor.openFile` (which calls `fresh --cmd session open-file`) fails silently — Fresh's internal session server isn't ready to accept commands yet even though the PTY status is 'running'. Needs investigation into the gap between PTY running and Fresh session server accepting commands (socket/pipe readiness). Possible fixes: poll Fresh's session socket directly, add a ready-check command to Fresh, or increase wait time with Fresh-specific readiness probe

### Language Support

- [ ] Add first-party Bash language support extension
- [ ] Add first-party Fish language support extension
- [ ] Add first-party Go language support extension
- [ ] Add first-party Java language support extension
- [ ] Add first-party JSON language support extension
- [ ] Add first-party JSX / React language support extension
- [ ] Add first-party Markdown language support extension — likely needs more than LSP alone (preview, structure, authoring helpers)
- [ ] Add first-party Rust language support extension
- [ ] Add first-party TOML language support extension
- [ ] Add first-party YAML language support extension

### Editor State & Tabs

- [ ] Re-open existing file tabs instead of duplicating them — clicking a file that is already open should always focus the existing tab, not create another temp/permanent copy of the same file
- [ ] Fix temp-to-permanent promotion without resetting Monaco state — promoting a temp tab to permanent should preserve model state, undo stack, dirty state, and cursor position
- [ ] Preserve Monaco state when moving tabs between panes — dragging a dirty editor tab to another pane currently resets the file contents and loses undo/redo history
- [ ] Close or replace the placeholder "new tab" when opening a real file/session/tab — likely easiest if the blank new tab is always temporary
- [ ] Keyboard focus should follow newly opened tabs — opening a file/tab should move editor focus for typing, not just select the tab visually
- [ ] Reload clean Monaco models when files change on disk — if an open file has no local edits and the file changes externally, refresh the editor contents instead of leaving stale text in memory
- [ ] Show git change gutters in Monaco — line-level added/modified/deleted markers in the editor margin

### Diff View

- [ ] Re-focus already-open diff tabs correctly from the git file list — clicking the same diff entry again should navigate to the existing diff tab even after opening/scrolling elsewhere in the diff view
- [ ] Investigate editable diff mode — current diff editors are read-only, but we may want an explicit edit/apply workflow instead of only read-only inspection

### Markdown

- [ ] Notes sidebar — new activity bar view for project markdown notes. Auto-discovers common repo files (README.md, TODO.md, CONTRIBUTING.md, CHANGELOG.md) and pins them at the top. Below that, lists all other .md/.mdx files in the repo. Clicking any file opens it in the built-in markdown editor
- [ ] Checklist support — render `- [ ]` / `- [x]` as interactive checkboxes in preview that toggle the source on click
- [ ] Relative image preview — resolve relative image paths against the project root so images in markdown files render correctly in preview
- [ ] Create new note — button in the notes sidebar to create a new .md file in the project

### Project & Session Recovery

- [ ] Workspace view-state persistence — pane layout, active tabs, and app-shell reopen now persist; remaining gaps are sidebar view, collapsed dirs, and other non-tab UI state
- [x] App-level tab restoration — persisted content tabs now restore across reopened projects via app-shell + per-project workspace hydration
- [ ] Non-blocking restoration — load last state asynchronously so the app doesn't freeze if a session fails to reconnect
- [ ] Reload View dirty check — the Reload View command (`window.location.reload()`) destroys all Monaco editors, terminal frontend state, and unsaved content with no confirmation. Needs a dirty-tab check and confirm dialog before reloading. Part of broader session recovery work

### File Handling & File Explorer

- [ ] Unsaved changes warning on tab close — closing a dirty editor tab silently discards edits. `closeTab` in workspace.svelte.ts has no awareness of dirty state. Needs confirm dialog before closing tabs with unsaved changes, and possibly a "Save All" or "Discard" flow
- [ ] Vykar repo backups (beta opt-in) — integrate Vykar backup functionality into the file explorer view. Opt-in feature behind a beta flag so users can enable/disable it. Lives in the files sidebar as a backup management section
- [x] Git status colors in file tree — Files sidebar now renders git status badges for changed files and marks directories containing changes
- [ ] Inline rename for files and folders — replace the PromptDialog-based rename with an in-tree input that takes over the node's label (like VS Code, Nautilus). Need to handle focus, Escape to cancel, Enter to commit, and path validation inline
- [ ] Paste safety cap for large clipboards — add a sanity limit to `file_paste` (e.g. 100k files / 1 GiB total) to prevent accidents when the clipboard points at a huge tree (`/`, `$HOME`). Proper UX is progress + cancel (depends on notification system below)
- [x] Paste collision UX — paste and drop now share Replace / Skip / Rename collision handling via `ImportCollisionDialog` and explicit backend collision policies
- [x] Drag-and-drop files in/out of the file explorer — in-app moves and external-in drops are now supported with shared collision UX; drag-out to external file managers is still deferred
- [ ] Symlink support in file operations — `copy_recursive` currently follows symlinks instead of preserving them, and Monaco errors with "Is a directory" when opening symlinks-to-directories. Need `std::os::unix::fs::symlink` in the copy path and Monaco-side detection before open
- [x] Surface file-op errors to the user — paste/rename/delete/cut/copy/new-item and git copy-patch/discard now route failures through `notify.error()`
- [ ] Incremental file tree updates — every paste/rename/delete/new currently triggers a full `backend.files.listAll(projectPath)` re-walk (git ls-files or filesystem scan up to 25k files). For large repos this will eventually feel sluggish. `file_paste` already returns the list of created project-relative paths; extend rename/delete/create to do the same, then mutate the in-memory `fileTree` directly instead of reloading
- [ ] File operations follow-through — the file context menu already supports rename/delete/cut/copy/paste/new item; remaining gaps are duplicate/move flows plus trash vs permanent-delete semantics

### Notifications & Feedback

- [x] Status-bar notification system — `src/lib/stores/notifications.svelte.ts` + `NotificationsButton` in the status bar with a bell icon, unread badge, and popover listing all notifications as Alerts
- [ ] Notification update API should allow clearing descriptions — `NotificationUpdate.description` can replace a description but cannot explicitly remove one once set. Add a clearable contract for updates so loading/success/error transitions can drop stale body copy without replacing the whole notification
- [ ] Real progress notifications + cancel wiring — the notification store already supports loading/progress/actions; remaining work is standardizing cancel semantics and wiring long-running operations like paste, clone, and push to use it
- [ ] Progress feedback for long file operations — visual indicator (spinner in sidebar, progress in status bar) during paste/copy of large trees. Today there's no feedback between click and completion
- [ ] Branch controls in file view — show current branch and provide quick-access branch switcher in the files sidebar header (or dropdown)
- [ ] File preview/viewer for common types — support previewing images (PNG, JPG, SVG), JSON, CSV, and other common file types inline in the editor pane, not just markdown

### Terminal / PTY

- [ ] PTY output ring buffer — keep last N bytes of output per session in PtyService so webview reloads can replay terminal history and reconnect to the running PTY instead of restarting the session
- [ ] Terminal font + settings cleanup — `terminal_font_family` and `terminal_font_size` exist on `GeneralSettings` (Rust models + SQLite) but `TerminalSessionManager.TERMINAL_OPTIONS` hardcodes `'Monocraft Nerd Font'` and never reads them. The Svelte-side Terminal settings view was removed (dead UI). Decide: either (a) wire the settings through so users can customize the terminal font + size, or (b) drop the fields from `GeneralSettings`, the SQLite schema, and any migration. Same question applies to anywhere else we "force mono" — confirm we want that locked in or make it configurable.
- [x] Drag-and-drop image attach into terminal — terminal drop target now handles file/os drops, image payloads are written to temp via `dnd_save_dropped_bytes`, and dropped paths are shell-quoted into the PTY
