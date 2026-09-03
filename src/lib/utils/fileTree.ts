/**
 * Converts a flat list of GitChange paths into a nested tree structure
 * for VS Code-style file tree rendering in the git panel.
 */

import type { GitChange } from '$lib/types/backend'

export interface FileTreeNode<T extends { path: string } = GitChange> {
  /** Segment name (e.g. "components" or "Foo.svelte") */
  name: string
  /** Full relative path from project root */
  path: string
  type: 'file' | 'directory'
  children: FileTreeNode<T>[]
  /** Only set on leaf file nodes */
  change?: T
  /**
   * Depth-first position in the tree, unique within it. Lets a per-query pass
   * (see `buildTreeFilter`) record a verdict per node in a flat array instead
   * of hashing paths. Assigned by `buildFileTree`; nodes built elsewhere must
   * keep it unique too.
   */
  index: number
  /** `name` lowercased once at build time, so filtering never re-lowercases. */
  lowerName: string
}

/**
 * Build a nested tree from flat paths.
 * Single-child directory chains are compacted (e.g. "src/lib" instead of "src" > "lib").
 */
export function buildFileTree<T extends { path: string }>(changes: T[]): FileTreeNode<T>[] {
  const root: FileTreeNode<T> = {
    name: '',
    path: '',
    type: 'directory',
    children: [],
    index: 0,
    lowerName: ''
  }

  for (const change of changes) {
    const segments = change.path.split('/')
    let current = root

    for (let i = 0; i < segments.length; i++) {
      const segment = segments[i]
      const isFile = i === segments.length - 1
      const partialPath = segments.slice(0, i + 1).join('/')

      let child: FileTreeNode<T> | undefined = current.children.find(
        (c) => c.name === segment && c.type === (isFile ? 'file' : 'directory')
      )

      if (!child) {
        child = {
          name: segment,
          path: partialPath,
          type: isFile ? 'file' : 'directory',
          children: [],
          change: isFile ? change : undefined,
          index: 0,
          lowerName: segment.toLowerCase()
        }
        current.children.push(child)
      }

      if (!isFile) {
        current = child
      }
    }
  }

  sortTree(root)
  const nodes = compactTree(root.children)
  indexTree(nodes, 0)
  return nodes
}

/** Number of nodes in a tree built by [`buildFileTree`]. */
export function treeNodeCount<T extends { path: string }>(nodes: FileTreeNode<T>[]): number {
  if (nodes.length === 0) return 0
  // Indexes are assigned depth-first, so the highest one sits at the end of
  // the last root's deepest trailing chain.
  let last = nodes[nodes.length - 1]
  while (last.children.length > 0) last = last.children[last.children.length - 1]
  return last.index + 1
}

function indexTree<T extends { path: string }>(nodes: FileTreeNode<T>[], next: number): number {
  for (const node of nodes) {
    node.index = next
    next = indexTree(node.children, next + 1)
  }
  return next
}

function sortTree(node: FileTreeNode<{ path: string }>): void {
  node.children.sort((a, b) => {
    if (a.type !== b.type) return a.type === 'directory' ? -1 : 1
    return a.name.localeCompare(b.name)
  })
  for (const child of node.children) {
    if (child.type === 'directory') sortTree(child)
  }
}

/**
 * Collapse directory chains where a directory has exactly one child
 * that is also a directory. "src" > "lib" becomes "src/lib".
 *
 * Fully immutable; returns new nodes rather than mutating inputs.
 */
function compactTree<T extends { path: string }>(nodes: FileTreeNode<T>[]): FileTreeNode<T>[] {
  return nodes.map((node) => {
    if (node.type !== 'directory') return node

    const compactedChildren = compactTree(node.children)

    if (compactedChildren.length === 1 && compactedChildren[0].type === 'directory') {
      const child = compactedChildren[0]
      return {
        ...child,
        name: `${node.name}/${child.name}`,
        lowerName: `${node.lowerName}/${child.lowerName}`
      }
    }

    return { ...node, children: compactedChildren }
  })
}

/** Count leaf files in a tree (for group header counts). */
export function countFiles(nodes: FileTreeNode<{ path: string }>[]): number {
  let count = 0
  for (const node of nodes) {
    if (node.type === 'file') count++
    else count += countFiles(node.children)
  }
  return count
}

/** A flat row produced by [`flattenVisibleTree`]; node + indent depth. */
export interface FlatTreeRow<T extends { path: string }> {
  node: FileTreeNode<T>
  depth: number
}

/**
 * Flatten a tree into the ordered sequence of rows that are currently
 * visible; i.e. the root nodes plus the children of any expanded
 * directory. Used to feed a virtualized renderer.
 *
 * `isCollapsed` receives the node rather than its path so callers can answer
 * from the node itself — the tree filter keeps its verdicts in an array keyed
 * by `index`, and this runs once per node.
 */
export function flattenVisibleTree<T extends { path: string }>(
  nodes: FileTreeNode<T>[],
  isCollapsed: (node: FileTreeNode<T>) => boolean
): FlatTreeRow<T>[] {
  const out: FlatTreeRow<T>[] = []
  const walk = (level: FileTreeNode<T>[], depth: number): void => {
    for (const node of level) {
      out.push({ node, depth })
      if (node.type === 'directory' && !isCollapsed(node)) {
        walk(node.children, depth + 1)
      }
    }
  }
  walk(nodes, 0)
  return out
}
