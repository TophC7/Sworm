// Agent activity store.
//
// Tracks per-session activity state derived from PTY output
// classification. Uses debouncing to prevent flicker: busy state is
// held for a minimum duration even if the output goes neutral briefly.
//
// Activity states:
//   working   - agent is thinking/executing (accent dot)
//   waiting   - agent waiting for user input (yellow dot)
//   completed - agent finished its work, PTY exited (green dot)
//   idle      - no activity / not running (no dot)
//
// Reactivity uses [`ReactiveMap`] so each session has its own version.
// A row dot in SessionHistoryView subscribes to its session's signal
// only and never wakes for a sibling session's chunk.

import { classifyActivity, type ActivitySignal } from '$lib/features/sessions/terminal/activityClassifier'
import { ReactiveMap } from '$lib/utils/reactiveMap.svelte'

// TYPES //

export type AgentActivity = 'working' | 'waiting' | 'completed' | 'idle'

interface ActivityEntry {
  signal: ActivitySignal
  activity: AgentActivity
  updatedAt: number
  // Busy hold timer: prevents flicker on brief neutral gaps.
  holdTimer: ReturnType<typeof setTimeout> | null
}

// CONSTANTS //

// Minimum ms to hold "busy" display after last busy signal.
const BUSY_HOLD_MS = 2_000

// After this many ms of neutral output, drop back to idle.
const NEUTRAL_TIMEOUT_MS = 6_000

// STATE //

const activities = new ReactiveMap<string, ActivityEntry>()

// HELPERS //

function getOrCreate(sessionId: string): ActivityEntry {
  let entry = activities.get(sessionId)
  if (!entry) {
    entry = {
      signal: 'neutral',
      activity: 'idle',
      updatedAt: Date.now(),
      holdTimer: null
    }
    activities.set(sessionId, entry)
  }
  return entry
}

// Mutate an entry's signal/activity fields in place, refresh
// `updatedAt`, and bump the session's signal so subscribed effects
// re-run. Use for every change that consumers may observe.
function updateEntry(sessionId: string, patch: Partial<Pick<ActivityEntry, 'signal' | 'activity'>>) {
  const entry = activities.get(sessionId)
  if (!entry) return
  Object.assign(entry, patch, { updatedAt: Date.now() })
  activities.bumpKey(sessionId)
}

function touchEntry(entry: ActivityEntry, signal: ActivitySignal) {
  entry.signal = signal
  entry.updatedAt = Date.now()
}

function clearHold(sessionId: string) {
  const entry = activities.get(sessionId)
  if (entry?.holdTimer) {
    clearTimeout(entry.holdTimer)
    entry.holdTimer = null
  }
}

// PUBLIC API //

/**
 * Feed a PTY output chunk for classification.
 * Called from TerminalSessionManager on every output callback.
 */
export function feedOutput(sessionId: string, providerId: string, chunk: string) {
  const signal = classifyActivity(providerId, chunk)
  const entry = getOrCreate(sessionId)

  if (signal === 'busy') {
    clearHold(sessionId)
    if (entry.activity !== 'working') {
      updateEntry(sessionId, { signal, activity: 'working' })
    } else {
      touchEntry(entry, signal)
    }
    return
  }

  if (signal === 'idle') {
    clearHold(sessionId)
    if (entry.activity !== 'waiting') {
      updateEntry(sessionId, { signal, activity: 'waiting' })
    } else {
      touchEntry(entry, signal)
    }
    return
  }

  // Neutral: if currently working, hold for BUSY_HOLD_MS before
  // dropping. The timer mutation itself is internal state; consumers
  // only read `.activity`, which doesn't change until the timer fires.
  if (entry.activity === 'working' && !entry.holdTimer) {
    entry.holdTimer = setTimeout(() => {
      const current = activities.get(sessionId)
      if (current && current.activity === 'working') {
        current.holdTimer = null
        updateEntry(sessionId, { signal: 'neutral', activity: 'idle' })
      }
    }, BUSY_HOLD_MS)
  }

  // If currently waiting, drop to idle after a longer timeout.
  if (entry.activity === 'waiting' && !entry.holdTimer) {
    entry.holdTimer = setTimeout(() => {
      const current = activities.get(sessionId)
      if (current && current.activity === 'waiting') {
        current.holdTimer = null
        updateEntry(sessionId, { signal: 'neutral', activity: 'idle' })
      }
    }, NEUTRAL_TIMEOUT_MS)
  }
}

/** Mark a session as completed (PTY exited cleanly). */
export function markCompleted(sessionId: string) {
  clearHold(sessionId)
  const entry = getOrCreate(sessionId)
  // Only transition completed if the session was actually doing something.
  if (entry.activity === 'working' || entry.activity === 'waiting') {
    updateEntry(sessionId, { signal: 'neutral', activity: 'completed' })
  }
}

/**
 * Get current activity for a session. Returns 'idle' if unknown.
 *
 * Subscribes the calling effect to this session's signal only;
 * activity changes for other sessions do not re-fire.
 */
export function getActivity(sessionId: string): AgentActivity {
  void activities.keyVersion(sessionId)
  return activities.get(sessionId)?.activity ?? 'idle'
}

/** Clean up timers for a session (call on session removal). */
export function removeSession(sessionId: string) {
  clearHold(sessionId)
  activities.delete(sessionId)
}
