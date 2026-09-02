// Agent activity store.
//
// Tracks per-session-tab activity state derived from PTY output
// classification. Uses debouncing to prevent flicker: busy state is
// held for a minimum duration even if the output goes neutral briefly.
//
// Activity states:
//   working   - agent is thinking/executing (accent dot)
//   waiting   - agent waiting for user input (yellow dot)
//   idle      - no activity / not running (no dot)
//
// Reactivity uses [`ReactiveMap`] so each tab has its own version.
// A session dot in the tab strip subscribes to its tab's signal
// only and never wakes for a sibling tab's chunk.

import { classifyActivity, type ActivitySignal } from '$lib/features/sessions/terminal/activityClassifier'
import type { TabId } from '$lib/features/workbench/model'
import { ReactiveMap } from '$lib/utils/reactiveMap.svelte'

// TYPES //

export type AgentActivity = 'working' | 'waiting' | 'idle'

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

const activities = new ReactiveMap<TabId, ActivityEntry>()

// HELPERS //

function getOrCreate(tabId: TabId): ActivityEntry {
  let entry = activities.get(tabId)
  if (!entry) {
    entry = {
      signal: 'neutral',
      activity: 'idle',
      updatedAt: Date.now(),
      holdTimer: null
    }
    activities.set(tabId, entry)
  }
  return entry
}

// Mutate an entry's signal/activity fields in place, refresh
// `updatedAt`, and bump the tab's signal so subscribed effects
// re-run. Use for every change that consumers may observe.
function updateEntry(tabId: TabId, patch: Partial<Pick<ActivityEntry, 'signal' | 'activity'>>) {
  const entry = activities.get(tabId)
  if (!entry) return
  Object.assign(entry, patch, { updatedAt: Date.now() })
  activities.bumpKey(tabId)
}

function touchEntry(entry: ActivityEntry, signal: ActivitySignal) {
  entry.signal = signal
  entry.updatedAt = Date.now()
}

function clearHold(tabId: TabId) {
  const entry = activities.get(tabId)
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
export function feedOutput(tabId: TabId, providerId: string, chunk: string) {
  const signal = classifyActivity(providerId, chunk)
  const entry = getOrCreate(tabId)

  if (signal === 'busy') {
    clearHold(tabId)
    if (entry.activity !== 'working') {
      updateEntry(tabId, { signal, activity: 'working' })
    } else {
      touchEntry(entry, signal)
    }
    return
  }

  if (signal === 'idle') {
    clearHold(tabId)
    if (entry.activity !== 'waiting') {
      updateEntry(tabId, { signal, activity: 'waiting' })
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
      const current = activities.get(tabId)
      if (current && current.activity === 'working') {
        current.holdTimer = null
        updateEntry(tabId, { signal: 'neutral', activity: 'idle' })
      }
    }, BUSY_HOLD_MS)
  }

  // If currently waiting, drop to idle after a longer timeout.
  if (entry.activity === 'waiting' && !entry.holdTimer) {
    entry.holdTimer = setTimeout(() => {
      const current = activities.get(tabId)
      if (current && current.activity === 'waiting') {
        current.holdTimer = null
        updateEntry(tabId, { signal: 'neutral', activity: 'idle' })
      }
    }, NEUTRAL_TIMEOUT_MS)
  }
}

/**
 * Get current activity for a session tab. Returns 'idle' if unknown.
 *
 * Subscribes the calling effect to this tab's signal only;
 * activity changes for other tabs do not re-fire.
 */
export function getActivity(tabId: TabId): AgentActivity {
  void activities.keyVersion(tabId)
  return activities.get(tabId)?.activity ?? 'idle'
}

/** Clean up timers for a session tab (call when its manager is disposed). */
export function removeSession(tabId: TabId) {
  clearHold(tabId)
  activities.delete(tabId)
}
