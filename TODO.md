# TODO

## UI

- [ ] Add shadcn-svelte Tooltip primitive and apply to zoom buttons with shortcut hints (e.g. "Zoom in (Ctrl+=)"), window controls, and other icon-only actions

## Git Stash UX

- [ ] Stash operation error handling — `stash pop` can fail with merge conflicts leaving the working tree in a conflicted state, `stash drop` can also fail. Currently errors are silently caught and logged to console. Needs proper conflict detection, user-visible error states, and guided recovery (not just a toast — the UI should reflect the conflicted state and offer resolution paths)
- [ ] Stash action button states — Pop/Drop/Apply buttons have no in-flight/disabled state during async operations. Rapid clicks can operate on shifted indices after a pop. Needs operation locking, optimistic UI updates, or at minimum disabling buttons while an operation is pending. Ties into the error handling above since both need a coherent "operation in progress" model

## Terminal / PTY

- [ ] PTY output ring buffer — keep last N bytes of output per session in PtyService so webview reloads can replay terminal history and reconnect to the running PTY instead of restarting the session
