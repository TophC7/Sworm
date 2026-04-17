// Dirty-editor registry — tracks which editor buffers have unsaved
// changes so the managed reload / close paths can warn the user before
// losing in-memory edits.
//
// Per the recovery spec dirty content is intentionally NOT persisted
// across crashes in V1 — we only protect against accidental reloads.

// Svelte 5 proxies Sets for deep reactivity, so in-place add/delete
// is both reactive and allocation-free — one keystroke shouldn't
// churn the GC.
const dirtyKeys = $state<Set<string>>(new Set())

function makeKey(projectId: string, filePath: string): string {
  return `${projectId}::${filePath}`
}

export function setEditorDirty(projectId: string, filePath: string, dirty: boolean): void {
  const key = makeKey(projectId, filePath)
  const has = dirtyKeys.has(key)
  if (dirty === has) return
  if (dirty) dirtyKeys.add(key)
  else dirtyKeys.delete(key)
}

export function clearEditorDirty(projectId: string, filePath: string): void {
  setEditorDirty(projectId, filePath, false)
}

export function hasAnyDirtyEditors(): boolean {
  return dirtyKeys.size > 0
}

export function getDirtyEditorsCount(): number {
  return dirtyKeys.size
}

export function isEditorDirty(projectId: string, filePath: string): boolean {
  return dirtyKeys.has(makeKey(projectId, filePath))
}
