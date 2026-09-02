// Session status visuals.
//
// Shared so the tab strip and any future session surface render the
// same status dot instead of each hand-rolling the mapping.
// Mirrors features/issues/visual.ts.

import { getActivity } from '$lib/features/sessions/state/sessionActivity.svelte'
import { isProcessLive, type SessionTab } from '$lib/features/workbench/model'

/**
 * Tailwind background class for a session tab's status dot. Running sessions
 * refine by live agent activity, so the call subscribes to that session's
 * activity signal.
 */
export function sessionDotClass(tab: SessionTab): string {
  if (tab.status === 'failed') return 'bg-danger'

  if (isProcessLive(tab.status)) {
    const activity = getActivity(tab.sessionId)
    if (activity === 'working') return 'bg-accent'
    if (activity === 'waiting') return 'bg-warning'
    return 'bg-success'
  }

  return 'bg-muted'
}
