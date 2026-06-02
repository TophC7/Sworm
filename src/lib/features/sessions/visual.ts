// Session status visuals.
//
// Shared so the projects sidebar and any future session surface render
// the same status dot and legend instead of each hand-rolling the
// mapping. Mirrors features/issues/visual.ts.

import { getActivity } from '$lib/features/sessions/state/sessionActivity.svelte'
import type { Session } from '$lib/types/backend'

/**
 * Tailwind background class for a session's status dot. Pairs with
 * SESSION_STATUS_LEGEND. Running sessions refine by live agent activity,
 * so the call subscribes to that session's activity signal.
 */
export function sessionDotClass(session: Session): string {
  if (session.status === 'failed') return 'bg-danger'

  if (session.status === 'running' || session.status === 'starting') {
    const activity = getActivity(session.id)
    if (activity === 'working') return 'bg-accent'
    if (activity === 'waiting') return 'bg-warning'
    return 'bg-success'
  }

  if (session.status === 'idle') return 'bg-success'

  return 'bg-muted'
}

export interface SessionStatusLegendItem {
  dot: string
  label: string
}

export const SESSION_STATUS_LEGEND: SessionStatusLegendItem[] = [
  { dot: 'bg-success', label: 'Open / completed' },
  { dot: 'bg-accent', label: 'Agent working' },
  { dot: 'bg-warning', label: 'Waiting for input' },
  { dot: 'bg-danger', label: 'Failed' },
  { dot: 'bg-muted', label: 'Closed / exited' }
]
