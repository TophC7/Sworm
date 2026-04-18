# TODO

## 🔄 In Progress / Ongoing

### UI

- [ ] **Ongoing: keep CommandCenter updated** — whenever new features, views, or actions are added to the app, register them as commands in CommandCenter so they're discoverable via the command palette and keyboard shortcuts. This includes file operations, git actions, workspace controls, etc.

### File Explorer

- [ ] **WIP: Single-file diff view** — "Open Changes" in the git context menu should open a Monaco-based editable diff view for a single file (working copy vs HEAD). Currently disabled in GitContextMenu
- [ ] **WIP: Per-file stash** — "Stash Changes" in the git context menu should stash individual files (`git stash push -- <file>`). Currently disabled in GitContextMenu
- [ ] **WIP: Folder-scoped diff view** — "Open Changes" on a folder in the git context menu should open a stacked diff view for only files under that directory. Currently disabled in GitContextMenu

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
- [ ] Open in Fresh from editor toolbar — the "Open in Fresh" icon button in FileEditor's toolbar doesn't work when no Fresh session is pre-existing. `ensureFreshSession` creates the session and `waitForSessionReady` confirms the PTY is running, but `backend.editor.openFile` (which calls `fresh --cmd session open-file`) fails silently — Fresh's internal session server isn't ready to accept commands yet even though the PTY status is 'running'. Needs investigation into the gap between PTY running and Fresh session server accepting commands (socket/pipe readiness). Possible fixes: poll Fresh's session socket directly, add a ready-check command to Fresh, or increase wait time with Fresh-specific readiness probe

### Markdown

- [ ] Notes sidebar — new activity bar view for project markdown notes. Auto-discovers common repo files (README.md, TODO.md, CONTRIBUTING.md, CHANGELOG.md) and pins them at the top. Below that, lists all other .md/.mdx files in the repo. Clicking any file opens it in the built-in markdown editor
- [ ] Checklist support — render `- [ ]` / `- [x]` as interactive checkboxes in preview that toggle the source on click
- [ ] Relative image preview — resolve relative image paths against the project root so images in markdown files render correctly in preview
- [ ] Create new note — button in the notes sidebar to create a new .md file in the project

### Project & Session Recovery

- [ ] Persistent workspace state — save and restore pane layout, active tabs, and view state (sidebar view, collapsed dirs) per project so reopening a project restores what you were working on
- [ ] App-level tab restoration — when Sworm exits with tabs open across any project, restore them on app relaunch. Currently inconsistent — some tabs persist, some don't
- [ ] Non-blocking restoration — load last state asynchronously so the app doesn't freeze if a session fails to reconnect
- [ ] Reload View dirty check — the Reload View command (`window.location.reload()`) destroys all Monaco editors, terminal frontend state, and unsaved content with no confirmation. Needs a dirty-tab check and confirm dialog before reloading. Part of broader session recovery work

### File Handling & File Explorer

- [ ] Unsaved changes warning on tab close — closing a dirty editor tab silently discards edits. `closeTab` in workspace.svelte.ts has no awareness of dirty state. Needs confirm dialog before closing tabs with unsaved changes, and possibly a "Save All" or "Discard" flow
- [ ] Vykar repo backups (beta opt-in) — integrate Vykar backup functionality into the file explorer view. Opt-in feature behind a beta flag so users can enable/disable it. Lives in the files sidebar as a backup management section
- [ ] Git status colors in file tree — show file status (modified, staged, untracked, ignored) as inline badges or text color in the file explorer so you can see what's changed without switching to git view
- [ ] Inline rename for files and folders — replace the PromptDialog-based rename with an in-tree input that takes over the node's label (like VS Code, Nautilus). Need to handle focus, Escape to cancel, Enter to commit, and path validation inline
- [ ] Paste safety cap for large clipboards — add a sanity limit to `file_paste` (e.g. 100k files / 1 GiB total) to prevent accidents when the clipboard points at a huge tree (`/`, `$HOME`). Proper UX is progress + cancel (depends on notification system below)
- [x] Paste collision UX — paste and drop now share Replace / Skip / Rename collision handling via `ImportCollisionDialog` and explicit backend collision policies
- [x] Drag-and-drop files in/out of the file explorer — in-app moves and external-in drops are now supported with shared collision UX; drag-out to external file managers is still deferred
- [ ] Symlink support in file operations — `copy_recursive` currently follows symlinks instead of preserving them, and Monaco errors with "Is a directory" when opening symlinks-to-directories. Need `std::os::unix::fs::symlink` in the copy path and Monaco-side detection before open
- [x] Surface file-op errors to the user — paste/rename/delete/cut/copy/new-item and git copy-patch/discard now route failures through `notify.error()`
- [ ] Incremental file tree updates — every paste/rename/delete/new currently triggers a full `backend.files.listAll(projectPath)` re-walk (git ls-files or filesystem scan up to 25k files). For large repos this will eventually feel sluggish. `file_paste` already returns the list of created project-relative paths; extend rename/delete/create to do the same, then mutate the in-memory `fileTree` directly instead of reloading

### Notifications & Feedback

- [x] Status-bar notification system — `src/lib/stores/notifications.svelte.ts` + `NotificationsButton` in the status bar with a bell icon, unread badge, and popover listing all notifications as Alerts
- [ ] Notification update API should allow clearing descriptions — `NotificationUpdate.description` can replace a description but cannot explicitly remove one once set. Add a clearable contract for updates so loading/success/error transitions can drop stale body copy without replacing the whole notification
- [ ] Progress notifications with cancel — extend the notification store to support in-progress items (spinner + % complete + cancel action) for long-running operations like paste, clone, push. Currently notifications are one-shot info/success/warning/error only
- [ ] Progress feedback for long file operations — visual indicator (spinner in sidebar, progress in status bar) during paste/copy of large trees. Today there's no feedback between click and completion
- [ ] Branch controls in file view — show current branch and provide quick-access branch switcher in the files sidebar header (or dropdown)
- [ ] File preview/viewer for common types — support previewing images (PNG, JPG, SVG), JSON, CSV, and other common file types inline in the editor pane, not just markdown
- [ ] File operations — right-click menu to delete, rename, duplicate, or move files. Trash deleted files or permanent delete option. Should integrate with git if applicable

### Terminal / PTY

- [ ] PTY output ring buffer — keep last N bytes of output per session in PtyService so webview reloads can replay terminal history and reconnect to the running PTY instead of restarting the session
- [x] Drag-and-drop image attach into terminal — terminal drop target now handles file/os drops, image payloads are written to temp via `dnd_save_dropped_bytes`, and dropped paths are shell-quoted into the PTY
