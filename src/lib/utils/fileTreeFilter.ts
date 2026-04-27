import type { FileTreeNode } from './fileTree'

export interface TreeFilter {
  /** Paths that match the query directly or are ancestors of a match. Non-matches get dimmed. */
  matched: Set<string>
  /** Directory paths to auto-expand so the user can see matched leaves. */
  expand: Set<string>
}

const EMPTY: TreeFilter = { matched: new Set(), expand: new Set() }

/**
 * Compute which tree rows should remain at full opacity for a given
 * filter query, plus which directories must be expanded so matched
 * leaves are reachable.
 *
 * Matching is case-insensitive substring on each segment. We test every
 * segment (not just the basename) so typing `lib` highlights the
 * `src/lib` directory — which mirrors VSCode's Explorer "Filter on
 * Type" behavior. Matched paths and every ancestor directory get
 * marked so the visible chain stays bright.
 */
export function buildTreeFilter(nodes: FileTreeNode<{ path: string }>[], query: string): TreeFilter {
  const q = query.trim().toLowerCase()
  if (q.length === 0) return EMPTY

  const matched = new Set<string>()
  const expand = new Set<string>()

  function addAncestors(path: string): void {
    // Compacted directories collapse multiple segments into one node
    // (e.g. "src/lib"), so we walk every prefix of every internal slash
    // and let the caller intersect with the actual tree's path set.
    const parts = path.split('/')
    for (let i = 1; i < parts.length; i++) {
      const ancestor = parts.slice(0, i).join('/')
      matched.add(ancestor)
      expand.add(ancestor)
    }
  }

  function visit(node: FileTreeNode<{ path: string }>): boolean {
    let selfOrChildMatched = false

    if (node.type === 'directory') {
      let anyChildMatched = false
      for (const child of node.children) {
        if (visit(child)) anyChildMatched = true
      }
      if (anyChildMatched) {
        matched.add(node.path)
        expand.add(node.path)
        selfOrChildMatched = true
      }
    }

    // A node also matches in its own right when any of its path
    // segments contain the query — useful for compacted dir nodes
    // ("src/lib") that the user may want to spot directly.
    const segments = node.path.split('/')
    for (const seg of segments) {
      if (seg.toLowerCase().includes(q)) {
        matched.add(node.path)
        addAncestors(node.path)
        selfOrChildMatched = true
        break
      }
    }

    return selfOrChildMatched
  }

  for (const root of nodes) visit(root)
  return { matched, expand }
}
