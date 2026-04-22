// Shared ref-formatting helpers for git graph and tooltip components.

/** Strip git ref decoration prefixes, returning a display label. */
export function refLabel(ref: string): string {
  if (ref.startsWith('HEAD -> ')) return ref.slice(8)
  if (ref.startsWith('tag: ')) return ref.slice(5)
  return ref
}

/** Filter out the bare "HEAD" ref (shown implicitly via "HEAD -> branch"). */
export function visibleRefs(refs: string[]): string[] {
  return refs.filter((r) => r !== 'HEAD')
}
