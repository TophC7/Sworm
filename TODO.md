# TODO

## UI

- [ ] Add shadcn-svelte Tooltip primitive and apply to zoom buttons with shortcut hints (e.g. "Zoom in (Ctrl+=)"), window controls, and other icon-only actions

## Terminal / PTY

- [ ] PTY output ring buffer — keep last N bytes of output per session in PtyService so webview reloads can replay terminal history and reconnect to the running PTY instead of restarting the session
