// Dirty-editor registry — tracks which editor buffers have unsaved
// changes so the managed reload / close paths can warn the user before
// losing in-memory edits.
//
// Keyed by tabId rather than filePath so unsaved "Untitled" buffers
// (which have no filePath yet) participate in dirty tracking the same
// way as normal editor tabs. The projectId is retained in the key so
// dirty state from a closed project can't linger in the Set.
//
// Per the recovery spec dirty content is intentionally NOT persisted
// across crashes in V1 — we only protect against accidental reloads.

// Svelte 5 proxies Sets for deep reactivity, so in-place add/delete
// is both reactive and allocation-free — one keystroke shouldn't
// churn the GC.
const dirtyKeys = $state<Set<string>>(new Set())

function makeKey(projectId: string, tabId: string): string {
  return `${projectId}::${tabId}`
}

export function setEditorDirty(projectId: string, tabId: string, dirty: boolean): void {
  const key = makeKey(projectId, tabId)
  const has = dirtyKeys.has(key)
  if (dirty === has) return
  if (dirty) dirtyKeys.add(key)
  else dirtyKeys.delete(key)
}

export function clearEditorDirty(projectId: string, tabId: string): void {
  setEditorDirty(projectId, tabId, false)
}

export function hasAnyDirtyEditors(): boolean {
  return dirtyKeys.size > 0
}

export function getDirtyEditorsCount(): number {
  return dirtyKeys.size
}

export function isEditorDirty(projectId: string, tabId: string): boolean {
  return dirtyKeys.has(makeKey(projectId, tabId))
}
