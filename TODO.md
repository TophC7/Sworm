# TODO

## In Progress

### UI

- [ ] Ongoing: keep CommandCenter updated
  Register new features, views, and actions as commands so they are discoverable in the command palette and keyboard shortcuts.

### File Explorer

- [ ] WIP: Per-file stash
  Make "Stash Changes" in the git context menu stash individual files with `git stash push -- <file>`.

## Backlog

### UI

- [ ] Recursive split-tree panes with size + count caps
  Replace the fixed `SplitMode` / `QuadLayout` state machine in `workspace.svelte.ts` with a recursive `Node = Leaf | Branch` tree.
  Gate splits with:
  - measured minimum pane size via `getBoundingClientRect()` and `MIN_PANE_PX` around 240px
  - a hard `MAX_PANES` count cap around 8
  Update `PaneGrid.svelte`, `Pane.svelte`, and the pane drop observer, plus the workspace persistence migration.
- [ ] Pane overscroll feature
  Allow elastic overscroll when scrolling past the end of a pane.
  Start with a single-pane prototype and settle the threshold, max distance, trigger input, and return behavior before expanding it.

### Keyboard Shortcuts

- [ ] Allow binding shortcuts to commands that do not have one by default
  Widen the Command Center pencil action so any rebindable command can be assigned a shortcut.
- [ ] Add a Command Center row for `toggle-command-palette`
  Surface the palette command itself so the rebind UI can reach it.

### Git Stash UX

- [ ] Stash operation error handling
  Handle `stash pop` and `stash drop` failures with visible conflict states and recovery guidance.
- [ ] Stash action button states
  Add in-flight or disabled states for Pop / Drop / Apply so rapid clicks cannot race shifted stash indices.

### Editor

- [ ] Editor settings
  Expose Monaco options and editor extensions as user-configurable settings.
  Start with `renderWhitespace`, indent rainbow, `tabSize`, `fontSize`, and `wordWrap`.
- [ ] Bundled LSP runtime support
  Add runtime support for `bundled_binary` and `bundled_js` extension manifests by packaging server assets and resolving them at runtime.
- [ ] Open in Fresh from editor toolbar
  Fix the gap between session readiness and Fresh accepting commands when opening a file from the toolbar.

### Language Support

- [ ] Add first-party Bash language support extension
- [ ] Add first-party Fish language support extension
- [ ] Add first-party Go language support extension
- [ ] Add first-party Java language support extension
- [ ] Add first-party JSON language support extension
- [ ] Add first-party JSX / React language support extension
- [ ] Add first-party Markdown language support extension
- [ ] Add first-party Rust language support extension
- [ ] Add first-party TOML language support extension
- [ ] Add first-party YAML language support extension

### Editor State & Tabs

- [ ] Re-open existing file tabs instead of duplicating them
- [ ] Fix temp-to-permanent promotion without resetting Monaco state
- [ ] Preserve Monaco state when moving tabs between panes
- [ ] Close or replace the placeholder "new tab" when opening a real file, session, or tab
- [ ] Keyboard focus should follow newly opened tabs
- [ ] Reload clean Monaco models when files change on disk
- [ ] Show git change gutters in Monaco

### Diff View

- [ ] Re-focus already-open diff tabs correctly from the git file list
- [ ] Investigate editable diff mode
- [ ] Add a visual focus cue when opening a diff-stack file from the git tree
- [ ] Support selecting ranges inside the Monaco diff viewer so users can stage or unstage only the selected lines

### Markdown

- [ ] Notes sidebar
  Add a project notes activity bar view that pins common markdown files and lists the rest of the repo's `.md` / `.mdx` files.
- [ ] Checklist support
  Render `- [ ]` and `- [x]` as interactive checkboxes in preview.
- [ ] Relative image preview
  Resolve relative image paths against the project root in markdown preview.
- [ ] Create new note
  Add a button in the notes sidebar to create a new `.md` file.

### Project & Session Recovery

- [ ] Workspace view-state persistence
  Persist pane layout, active tabs, and remaining sidebar state such as collapsed directories.
- [x] App-level tab restoration
  Persisted content tabs now restore across reopened projects via app-shell and workspace hydration.
- [ ] Non-blocking restoration
  Load last state asynchronously so session recovery does not freeze the app.
- [ ] Reload View dirty check
  Warn before `window.location.reload()` destroys unsaved editors and terminal state.

### File Handling & File Explorer

- [ ] Unsaved changes warning on tab close
  Confirm before closing dirty editor tabs.
- [ ] Vykar repo backups (beta opt-in)
  Add an opt-in backup management section to the files sidebar.
- [ ] Inline rename for files and folders
  Replace the prompt dialog with an in-tree rename input.
- [ ] Paste safety cap for large clipboards
  Add a sanity limit to `file_paste` and pair it with progress and cancel UX.
- [ ] Symlink support in file operations
  Preserve symlinks during copy and handle symlinked directories correctly in Monaco.
- [ ] Incremental file tree updates
  Stop re-walking the entire tree after every file operation and mutate the in-memory tree directly.
- [ ] Configurable hidden files with sensible defaults
  Add explorer-level hidden or excluded path settings instead of hardcoded filtering.
  Ship defaults for `.bun`, `.git`, `.svelte-kit`, `node_modules`, `target`, `dist`, `build`, `.next`, `.nuxt`, `__pycache__`, `.cache`, `.venv`, and `venv`.
- [ ] File operations follow-through
  Finish duplicate and move flows, plus trash versus permanent-delete semantics.

### Notifications & Feedback

- [ ] Notification update API should allow clearing descriptions
  Allow updates to explicitly remove description text.
- [ ] Real progress notifications + cancel wiring
  Standardize cancel semantics and wire long-running operations through the notification store.
- [ ] Progress feedback for long file operations
  Show visible progress during paste and copy operations.
- [ ] Branch controls in file view
  Show current branch and add quick branch switching in the files sidebar.
- [ ] File preview/viewer for common types
  Add inline preview support for images, JSON, CSV, and other common types.

### Terminal / PTY

- [ ] PTY output ring buffer
  Keep recent terminal output so reloads can replay history and reconnect cleanly.
- [ ] Terminal font + settings cleanup
  Either wire terminal font and size settings through or remove the dead fields from the schema.
