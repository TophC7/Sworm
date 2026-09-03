import { treeNodeCount, type FileTreeNode } from './fileTree'

/** Row matched the query, or something beneath it did; stays at full opacity. */
const MATCH = 1
/** Directory holds a match somewhere below and must be open to reach it. */
const MATCH_BELOW = 2

export interface TreeFilter {
  isMatch: (node: { index: number }) => boolean
  shouldExpand: (node: { index: number }) => boolean
}

const EMPTY: TreeFilter = { isMatch: () => false, shouldExpand: () => false }

/**
 * Decide which tree rows stay at full opacity for a given filter query, and
 * which directories must be expanded so matched leaves are reachable.
 *
 * Matching is case-insensitive substring on each path segment, not just the
 * basename, so typing `lib` highlights `src/lib` and everything inside it —
 * mirroring VS Code's Explorer "Filter on Type" behavior. A segment match is
 * inherited downwards, which is what makes a matched directory's contents
 * count as matches too.
 *
 * This runs on every keystroke over every node, so it touches no strings it
 * doesn't have to: names are lowercased once at build time, and verdicts land
 * in one flat array keyed by `node.index` rather than in sets of paths.
 */
export function buildTreeFilter<T extends { path: string }>(nodes: FileTreeNode<T>[], query: string): TreeFilter {
  const q = query.trim().toLowerCase()
  if (q.length === 0) return EMPTY

  const flags = new Uint8Array(treeNodeCount(nodes))

  // A compacted directory's `lowerName` carries its whole run ("src/lib"), so
  // testing names down the chain covers every segment of every path.
  const visit = (node: FileTreeNode<T>, ancestorMatched: boolean): boolean => {
    const matched = ancestorMatched || node.lowerName.includes(q)
    let below = false
    for (const child of node.children) {
      if (visit(child, matched)) below = true
    }
    if (matched || below) flags[node.index] |= MATCH
    if (below) flags[node.index] |= MATCH_BELOW
    return matched || below
  }
  for (const root of nodes) visit(root, false)

  return {
    isMatch: (node) => (flags[node.index] & MATCH) !== 0,
    shouldExpand: (node) => (flags[node.index] & MATCH_BELOW) !== 0
  }
}
