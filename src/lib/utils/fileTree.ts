/**
 * Converts a flat list of GitChange paths into a nested tree structure
 * for VS Code-style file tree rendering in the git panel.
 */

import type { GitChange } from '$lib/types/backend'

export interface FileTreeNode {
  /** Segment name (e.g. "components" or "Foo.svelte") */
  name: string
  /** Full relative path from project root */
  path: string
  type: 'file' | 'directory'
  children: FileTreeNode[]
  /** Only set on leaf file nodes */
  change?: GitChange
}

/**
 * Build a nested tree from flat GitChange paths.
 * Single-child directory chains are compacted (e.g. "src/lib" instead of "src" > "lib").
 */
export function buildFileTree(changes: GitChange[]): FileTreeNode[] {
  const root: FileTreeNode = {
    name: '',
    path: '',
    type: 'directory',
    children: []
  }

  for (const change of changes) {
    const segments = change.path.split('/')
    let current = root

    for (let i = 0; i < segments.length; i++) {
      const segment = segments[i]
      const isFile = i === segments.length - 1
      const partialPath = segments.slice(0, i + 1).join('/')

      let child = current.children.find((c) => c.name === segment && c.type === (isFile ? 'file' : 'directory'))

      if (!child) {
        child = {
          name: segment,
          path: partialPath,
          type: isFile ? 'file' : 'directory',
          children: [],
          change: isFile ? change : undefined
        }
        current.children.push(child)
      }

      if (!isFile) {
        current = child
      }
    }
  }

  sortTree(root)
  return compactTree(root.children)
}

function sortTree(node: FileTreeNode): void {
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
 * Fully immutable — returns new nodes rather than mutating inputs.
 */
function compactTree(nodes: FileTreeNode[]): FileTreeNode[] {
  return nodes.map((node) => {
    if (node.type !== 'directory') return node

    const compactedChildren = compactTree(node.children)

    if (compactedChildren.length === 1 && compactedChildren[0].type === 'directory') {
      const child = compactedChildren[0]
      return {
        ...child,
        name: `${node.name}/${child.name}`
      }
    }

    return { ...node, children: compactedChildren }
  })
}

/** Count leaf files in a tree (for group header counts). */
export function countFiles(nodes: FileTreeNode[]): number {
  let count = 0
  for (const node of nodes) {
    if (node.type === 'file') count++
    else count += countFiles(node.children)
  }
  return count
}
