// Slash-grouping for the Branches view tree mode.
//
// Port of GitLens' `makeHierarchical`, scaled down to what Sworm needs.
// Splits each branch name on `/` and folds shared prefixes into shared
// nodes so `feature/auth/login` and `feature/auth/signup` collapse
// under one `feature` → `auth` chain.
//
// Edge case: a literal branch named `feature` alongside `feature/x`.
// We don't introduce a virtual separator; the `feature` parent gains
// a leaf child whose name is `(self)`, marked with `isSelf = true`,
// so the row is selectable.

import type { BranchSummary } from '$lib/types/backend'

export interface BranchTreeNode {
  /** Display name for this segment. For self-leaves, the literal
   * string `(self)`; for regular nodes, the path segment. */
  name: string
  /** Full slash path from root to this node (e.g. `feature/auth`). */
  fullName: string
  /** When this node corresponds to a real branch (leaf or self), the
   * underlying summary. Folder nodes have `branch === undefined`. */
  branch?: BranchSummary
  /** True when this leaf was synthesised because the parent name was
   * also a real branch. The row renders `(self)` so the literal
   * branch is still selectable. */
  isSelf?: boolean
  children: BranchTreeNode[]
}

export interface BranchTree {
  roots: BranchTreeNode[]
}

const SELF_LABEL = '(self)'

/**
 * Group a flat branch list into a tree by splitting names on `/`.
 *
 * Order semantics: root-level nodes follow the input order of their
 * first occurrence; siblings within a folder follow the order of
 * their first occurrence as well. Sorting by date / alpha is the
 * caller's job; handle it on the input list.
 */
export function groupBySlash(branches: BranchSummary[]): BranchTree {
  const roots: BranchTreeNode[] = []
  const lookup = new Map<string, BranchTreeNode>()

  for (const branch of branches) {
    const segments = splitName(branch.name)
    if (segments.length === 0) continue

    let parentChildren = roots
    let path = ''
    for (let i = 0; i < segments.length; i++) {
      const segment = segments[i]
      path = path === '' ? segment : `${path}/${segment}`
      const isLeaf = i === segments.length - 1

      let node = lookup.get(path)
      if (!node) {
        node = { name: segment, fullName: path, children: [] }
        lookup.set(path, node)
        parentChildren.push(node)
      }

      if (isLeaf) {
        if (node.branch !== undefined || node.children.length > 0) {
          // Parent-and-child collision: another branch already
          // exists under this path as a folder. Attach a `(self)`
          // leaf so the literal branch still has its own row.
          ensureSelfLeaf(node, branch)
        } else {
          node.branch = branch
        }
      } else if (node.branch !== undefined && !hasSelfLeaf(node)) {
        // The folder node was created earlier as a leaf for another
        // branch (e.g. `feature` arrived before `feature/x`).
        // Promote the existing branch onto a `(self)` child so the
        // folder can hold real children below it.
        const owned = node.branch
        node.branch = undefined
        ensureSelfLeaf(node, owned)
      }

      parentChildren = node.children
    }
  }

  return { roots }
}

function splitName(name: string): string[] {
  // Treat consecutive slashes (`//`) as a single separator so we don't
  // emit empty intermediate nodes. Trim leading / trailing slashes for
  // the same reason; git itself rejects those names but defend anyway.
  return name.split('/').filter((segment) => segment.length > 0)
}

function hasSelfLeaf(node: BranchTreeNode): boolean {
  return node.children.some((child) => child.isSelf === true)
}

function ensureSelfLeaf(parent: BranchTreeNode, branch: BranchSummary): void {
  if (hasSelfLeaf(parent)) return
  parent.children.unshift({
    name: SELF_LABEL,
    fullName: parent.fullName,
    branch,
    isSelf: true,
    children: []
  })
}
