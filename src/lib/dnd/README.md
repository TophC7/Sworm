# Sworm DnD

Unified drag-and-drop runtime for Sworm.

## Payload Schema

- Internal drags stamp `application/vnd.sworm.item+json` on `dataTransfer`.
- Internal drags also write to `LocalTransfer` for `dragover` access.
- External Tauri OS drops are converted into the same payload shape.

```ts
type DragPayload = {
  source: 'internal' | 'external'
  items: Array<
    | { kind: 'tab'; tabId: string; projectId: string; sourcePaneSlot: PaneSlot }
    | { kind: 'file'; path: string; isDir: boolean; projectId: string }
    | { kind: 'git-change'; path: string; staged: boolean; projectId: string }
    | { kind: 'os-files'; paths: string[] }
  >
}
```

## Add A New Adapter

1. Create `src/lib/dnd/adapters/<feature>.ts`.
2. Use `dragObserver(...)` for HTML5 drop targets.
3. Use `DropRegistry.register(...)` so Tauri OS drops can route to it.
4. For drag sources, set `LocalTransfer` + `stampDataTransfer(...)` on `dragstart`.
5. Clear `LocalTransfer` on `dragend`.

## Active Adapters

- `adapters/pane.ts`: tab/file/os-file drops into panes with merge/split overlays.
- `adapters/tab-strip.ts`: tab drag source wiring.
- `adapters/file-tree.ts`: file explorer source + folder/root targets with delayed expand.
- `adapters/git.ts`: git change drag sources + staged/unstaged drop zones.
- `adapters/terminal.ts`: terminal drops (file paths + image temp-save path insert).
