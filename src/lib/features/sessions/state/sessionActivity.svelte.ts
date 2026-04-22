/**
 * Agent activity store.
 *
 * Tracks per-session activity state derived from PTY output classification.
 * Uses debouncing to prevent flicker: busy state is held for a minimum
 * duration even if the output goes neutral briefly.
 *
 * Activity states:
 *   working   — agent is thinking/executing (accent dot)
 *   waiting   — agent waiting for user input (yellow dot)
 *   completed — agent finished its work, PTY exited (green dot, dismisses on view)
 *   idle      — no activity / not running (no dot)
 */

import { classifyActivity, type ActivitySignal } from '$lib/features/sessions/terminal/activityClassifier'

// -- Types --

export type AgentActivity = 'working' | 'waiting' | 'completed' | 'idle'

interface ActivityEntry {
  signal: ActivitySignal
  activity: AgentActivity
  updatedAt: number
  /** Busy hold timer — prevents flicker on brief neutral gaps. */
  holdTimer: ReturnType<typeof setTimeout> | null
}

// -- Constants --

/** Minimum ms to hold "busy" display after last busy signal. */
const BUSY_HOLD_MS = 2_000

/** After this many ms of neutral output, drop back to idle. */
const NEUTRAL_TIMEOUT_MS = 6_000

// -- State --

let entries = $state<Map<string, ActivityEntry>>(new Map())

// -- Helpers --

function getOrCreate(sessionId: string): ActivityEntry {
  let entry = entries.get(sessionId)
  if (!entry) {
    entry = {
      signal: 'neutral',
      activity: 'idle',
      updatedAt: Date.now(),
      holdTimer: null
    }
    entries.set(sessionId, entry)
    entries = new Map(entries)
  }
  return entry
}

function updateEntry(sessionId: string, patch: Partial<ActivityEntry>) {
  const entry = entries.get(sessionId)
  if (!entry) return
  Object.assign(entry, patch, { updatedAt: Date.now() })
  entries = new Map(entries)
}

function clearHold(sessionId: string) {
  const entry = entries.get(sessionId)
  if (entry?.holdTimer) {
    clearTimeout(entry.holdTimer)
    entry.holdTimer = null
  }
}

// -- Public API --

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
      entry.signal = signal
      entry.updatedAt = Date.now()
    }
    return
  }

  if (signal === 'idle') {
    clearHold(sessionId)
    updateEntry(sessionId, { signal, activity: 'waiting' })
    return
  }

  // Neutral: if currently working, hold for BUSY_HOLD_MS before dropping
  if (entry.activity === 'working' && !entry.holdTimer) {
    entry.holdTimer = setTimeout(() => {
      const current = entries.get(sessionId)
      if (current && current.activity === 'working') {
        updateEntry(sessionId, { signal: 'neutral', activity: 'idle', holdTimer: null })
      }
    }, BUSY_HOLD_MS)
  }

  // If currently waiting, drop to idle after longer timeout
  if (entry.activity === 'waiting' && !entry.holdTimer) {
    entry.holdTimer = setTimeout(() => {
      const current = entries.get(sessionId)
      if (current && current.activity === 'waiting') {
        updateEntry(sessionId, { signal: 'neutral', activity: 'idle', holdTimer: null })
      }
    }, NEUTRAL_TIMEOUT_MS)
  }
}

/** Mark a session as completed (PTY exited cleanly). */
export function markCompleted(sessionId: string) {
  clearHold(sessionId)
  const entry = getOrCreate(sessionId)
  // Only mark completed if the session was actually doing something
  if (entry.activity === 'working' || entry.activity === 'waiting') {
    updateEntry(sessionId, { signal: 'neutral', activity: 'completed', holdTimer: null })
  }
}

/** Get current activity for a session. Returns 'idle' if unknown. */
export function getActivity(sessionId: string): AgentActivity {
  return entries.get(sessionId)?.activity ?? 'idle'
}

/** Clean up timers for a session (call on session removal). */
export function removeSession(sessionId: string) {
  clearHold(sessionId)
  entries.delete(sessionId)
  entries = new Map(entries)
}
