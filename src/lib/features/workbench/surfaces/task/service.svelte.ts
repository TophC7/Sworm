// Task surface presentation helpers.
//
// Pure display derivations used by the tab bar and presentation layer.
// Tab-opening and lifecycle orchestration live in
// $lib/features/tasks/service.svelte.ts.

import type { TaskTab } from '$lib/features/workbench/model'

export function getTaskTabTitle(tab: TaskTab): string {
  if (tab.status === 'exited' && tab.exitCode !== null && tab.exitCode !== 0) {
    return `${tab.label} (exit ${tab.exitCode})`
  }
  return tab.label
}

/**
 * Return the Lucide icon name to use for this task tab. Falls back to
 * a generic "terminal" icon when the task has no icon configured.
 */
export function getTaskTabIcon(tab: TaskTab): string {
  return tab.icon ?? 'terminal'
}
