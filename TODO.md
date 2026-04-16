# TODO

## UI

- [x] Add shadcn-svelte Tooltip primitive and apply to zoom buttons with shortcut hints (e.g. "Zoom in (Ctrl+=)"), window controls, and other icon-only actions
- [x] Stricter design system for sidebars and views — define shared layout components or documented patterns for sidebar headers (title + actions left, collapse right), content toolbars (unified style via `ContentToolbar`), and view chrome so new views match existing ones by construction, not by manual copying. Current pain points: file sidebar had refresh button misplaced vs git sidebar, markdown toolbar initially didn't match diff toolbar styling
- [ ] **Ongoing: keep CommandCenter updated** — whenever new features, views, or actions are added to the app, register them as commands in CommandCenter so they're discoverable via the command palette and keyboard shortcuts. This includes file operations, git actions, workspace controls, etc.
- [ ] Pane overscroll feature — when scrolling past the end of content in a pane, allow elastic overscroll that pushes content up so the end feels reachable. Should have configurable threshold, max overscroll amount, and spring animation on return. Blocked on: threshold definition (immediate vs amount), max distance cap, trigger input (wheel only vs keyboard/trackpad), return behavior (scroll up vs any scroll down), handling of edge cases (empty/short content, text selection at bottom, window resize while overscrolled, keyboard nav during overscroll, momentum scrolling). Start with single pane prototype, not all panes.

## Git Stash UX

- [ ] Stash operation error handling — `stash pop` can fail with merge conflicts leaving the working tree in a conflicted state, `stash drop` can also fail. Currently errors are silently caught and logged to console. Needs proper conflict detection, user-visible error states, and guided recovery (not just a toast — the UI should reflect the conflicted state and offer resolution paths)
- [ ] Stash action button states — Pop/Drop/Apply buttons have no in-flight/disabled state during async operations. Rapid clicks can operate on shifted indices after a pop. Needs operation locking, optimistic UI updates, or at minimum disabling buttons while an operation is pending. Ties into the error handling above since both need a coherent "operation in progress" model

## Editor / Fresh Integration

- [ ] Open in Fresh from editor toolbar — the "Open in Fresh" icon button in FileEditor's toolbar doesn't work when no Fresh session is pre-existing. `ensureFreshSession` creates the session and `waitForSessionReady` confirms the PTY is running, but `backend.editor.openFile` (which calls `fresh --cmd session open-file`) fails silently — Fresh's internal session server isn't ready to accept commands yet even though the PTY status is 'running'. Needs investigation into the gap between PTY running and Fresh session server accepting commands (socket/pipe readiness). Possible fixes: poll Fresh's session socket directly, add a ready-check command to Fresh, or increase wait time with Fresh-specific readiness probe

## Markdown

- [ ] Notes sidebar — new activity bar view for project markdown notes. Auto-discovers common repo files (README.md, TODO.md, CONTRIBUTING.md, CHANGELOG.md) and pins them at the top. Below that, lists all other .md/.mdx files in the repo. Clicking any file opens it in the built-in markdown editor
- [ ] ~~Richer editor — replace the plain textarea with CodeMirror (markdown mode) for syntax highlighting, bracket matching, undo history, and keybindings~~ Replaced by Monaco integration
- [ ] Checklist support — render `- [ ]` / `- [x]` as interactive checkboxes in preview that toggle the source on click
- [ ] Relative image preview — resolve relative image paths against the project root so images in markdown files render correctly in preview
- [ ] Create new note — button in the notes sidebar to create a new .md file in the project

## Project & Session Recovery

- [ ] Persistent workspace state — save and restore pane layout, active tabs, and view state (sidebar view, collapsed dirs) per project so reopening a project restores what you were working on
- [ ] App-level tab restoration — when Sworm exits with tabs open across any project, restore them on app relaunch. Currently inconsistent — some tabs persist, some don't
- [ ] Non-blocking restoration — load last state asynchronously so the app doesn't freeze if a session fails to reconnect
- [ ] Reload View dirty check — the Reload View command (`window.location.reload()`) destroys all Monaco editors, terminal frontend state, and unsaved content with no confirmation. Needs a dirty-tab check and confirm dialog before reloading. Part of broader session recovery work

## File Handling UX

- [ ] Unsaved changes warning on tab close — closing a dirty editor tab silently discards edits. `closeTab` in workspace.svelte.ts has no awareness of dirty state. Needs confirm dialog before closing tabs with unsaved changes, and possibly a "Save All" or "Discard" flow

## File Explorer

- [ ] Vykar repo backups (beta opt-in) — integrate Vykar backup functionality into the file explorer view. Opt-in feature behind a beta flag so users can enable/disable it. Lives in the files sidebar as a backup management section
- [ ] Git status colors in file tree — show file status (modified, staged, untracked, ignored) as inline badges or text color in the file explorer so you can see what's changed without switching to git view
- [ ] Per-file git actions — right-click menu on files to stage, unstage, discard, or view changes for individual files. Quick actions without opening the full diff view
- [ ] Branch controls in file view — show current branch and provide quick-access branch switcher in the files sidebar header (or dropdown)
- [ ] File preview/viewer for common types — support previewing images (PNG, JPG, SVG), JSON, CSV, and other common file types inline in the editor pane, not just markdown
- [ ] File operations — right-click menu to delete, rename, duplicate, or move files. Trash deleted files or permanent delete option. Should integrate with git if applicable

## Terminal / PTY

- [ ] PTY output ring buffer — keep last N bytes of output per session in PtyService so webview reloads can replay terminal history and reconnect to the running PTY instead of restarting the session

