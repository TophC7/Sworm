// Shared indent helper for sidebar tree rows (branches, issues, etc.).
// Rows render at `padding-left: TREE_INDENT_BASE_PX + depth * TREE_INDENT_STEP_PX`.

export const TREE_INDENT_BASE_PX = 10
export const TREE_INDENT_STEP_PX = 12

export function treeIndent(depth: number): string {
  return `${TREE_INDENT_BASE_PX + depth * TREE_INDENT_STEP_PX}px`
}

export function treeIndentValue(depth: number): number {
  return TREE_INDENT_BASE_PX + depth * TREE_INDENT_STEP_PX
}
