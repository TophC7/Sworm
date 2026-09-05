# Sworm DnD

Unified drag-and-drop runtime for Sworm.

## Payload Schema

- Internal drags stamp the serialized payload as `application/vnd.sworm.item+json`.
- They also stamp an empty kind marker: `application/vnd.sworm.kind.tab`,
  `application/vnd.sworm.kind.file`, or `application/vnd.sworm.kind.git-change`.
- Same-window drags additionally write to `LocalTransfer` for synchronous payload access.
- External Tauri OS drops are converted into the same payload shape.

```ts
type DragPayload = {
  source: 'internal' | 'external'
  items: Array<
    | { kind: 'tab'; tabId: string; sourceWindowLabel?: string }
    | { kind: 'file'; path: string; isDir: boolean; folderPath: string }
    | { kind: 'git-change'; path: string; staged: boolean; folderPath: string }
    | { kind: 'os-files'; paths: string[] }
  >
}
```

## Cross-Window Contract

Browsers may expose `DataTransfer.types` during `dragenter` and `dragover` while withholding
custom MIME data until `drop`. Targets must use the kind markers above to decide compatibility
during hover. They must not reject a drag because `LocalTransfer` is empty: that store is only
shared inside one webview.

At `drop`, targets use `LocalTransfer` when present; otherwise they parse
`application/vnd.sworm.item+json` from `DataTransfer`. Parsing is deliberately deferred until
then. OS files remain identified by `Files`. Terminal targets also accept file paths supplied
by remote webviews as `text/plain`.

## Add A New Adapter

1. Create `src/lib/features/dnd/adapters/<feature>.ts`.
2. Use `dragObserver(...)` for HTML5 drop targets.
3. Use `DropRegistry.register(...)` so Tauri OS drops can route to it.
4. For drag sources, set `LocalTransfer` + `stampDataTransfer(...)` on `dragstart`.
5. Clear `LocalTransfer` on `dragend`.

## Active Adapters

- `adapters/tab-strip.ts`: tab drag source wiring (title-bar tab reorder).
- `adapters/file-tree.ts`: file explorer source + folder/root targets with delayed expand.
- `adapters/git.ts`: git change drag sources + staged/unstaged drop zones.
- `adapters/terminal.ts`: terminal drops (file paths + image temp-save path insert).
