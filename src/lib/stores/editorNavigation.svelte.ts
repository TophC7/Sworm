type EditorRevealTarget =
  | {
      kind: 'range'
      startLineNumber: number
      startColumn: number
      endLineNumber: number
      endColumn: number
    }
  | {
      kind: 'position'
      lineNumber: number
      column: number
    }

const queuedReveals = new Map<string, EditorRevealTarget>()

function makeKey(projectId: string, uriPath: string): string {
  return `${projectId}::${uriPath}`
}

export function queueEditorReveal(projectId: string, uriPath: string, target: EditorRevealTarget) {
  queuedReveals.set(makeKey(projectId, uriPath), target)
}

export function consumeEditorReveal(projectId: string, uriPath: string): EditorRevealTarget | null {
  const key = makeKey(projectId, uriPath)
  const value = queuedReveals.get(key) ?? null
  if (value) queuedReveals.delete(key)
  return value
}
