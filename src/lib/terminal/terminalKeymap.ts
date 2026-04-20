// Terminal keymap — single source of truth for keys consumed inside a
// session terminal.
//
// The keyboard model in Sworm is layered:
//
//   Tier 1 — App shortcuts        keybindings.svelte.ts (window capture)
//   Tier 2 — Surface keymaps      this file + (future) editorKeymap.ts
//   Tier 3 — Component-local      inline onkeydown in dialogs / inputs
//
// Tier 1 fires first via a capture-phase window listener but yields to
// the shell whenever a terminal has DOM focus, unless the binding is
// tagged `skipShell` (palette, zoom, reload, project picker). Anything
// Tier 1 doesn't claim continues into the focused surface, where xterm's
// `attachCustomKeyEventHandler` calls `resolveTerminalKey()`. The
// returned `KeyAction` tells `TerminalSessionManager` exactly what to
// do — never split the decision between the keymap and the manager.
//
// Per-CLI quirks live in PROVIDER_PROFILES below. Adding a new agent
// or changing how an existing one expects Shift+Tab is a one-line
// table edit; the manager stays untouched.

/**
 * Outcome of inspecting a single keydown event. The terminal manager
 * is a switch over this discriminated union — no other branching.
 */
export type KeyAction =
  | { kind: 'pass' }
  | { kind: 'browser' }
  | { kind: 'send-pty'; bytes: string }
  | { kind: 'paste-text-or-image' }
  | { kind: 'paste-text-only' }
  | { kind: 'copy-or-sigint' }

/**
 * Per-provider behaviour. Add an entry only when an agent is known to
 * misbehave with the default; otherwise it falls through to
 * DEFAULT_PROFILE which lets xterm do its native encoding.
 */
interface ProviderProfile {
  /**
   * How to encode shift-modified Tab and Enter.
   *   'legacy' — `\x1b[Z` and `\x1b\r`. Required by agents that read
   *              keys via classic terminfo (Claude Code, Codex).
   *   'kitty'  — `\x1b[9;2u` and `\x1b[13;2u`. Required by agents that
   *              negotiate the kitty keyboard protocol and reject the
   *              legacy bytes (Gemini CLI).
   *   'native' — Do not intercept. xterm encodes per its current
   *              kitty-mode state, which is correct for plain shells
   *              and most TUIs.
   */
  shiftEncoding: 'legacy' | 'kitty' | 'native'
}

const DEFAULT_PROFILE: ProviderProfile = { shiftEncoding: 'native' }

const PROVIDER_PROFILES: Record<string, ProviderProfile> = {
  claude_code: { shiftEncoding: 'legacy' },
  codex: { shiftEncoding: 'legacy' },
  gemini: { shiftEncoding: 'kitty' }
  // copilot, crush, fresh, terminal: rely on DEFAULT_PROFILE.
}

function profileFor(providerId: string | null): ProviderProfile {
  if (!providerId) return DEFAULT_PROFILE
  return PROVIDER_PROFILES[providerId] ?? DEFAULT_PROFILE
}

const PASS: KeyAction = { kind: 'pass' }
const BROWSER: KeyAction = { kind: 'browser' }

/**
 * Decide what should happen with a keydown delivered to xterm.
 *
 * Rules, in order:
 *   1. Non-keydown events pass through.
 *   2. Ctrl/Cmd+Shift+C → browser copy (so OS clipboard gets the
 *      selection instead of the agent receiving Ctrl+C).
 *   3. Ctrl/Cmd+C (no shift) → copy if a selection exists, otherwise
 *      let xterm send SIGINT. Manager inspects the terminal state.
 *   4. Ctrl/Cmd+V        → clipboard paste, with image-fallback.
 *      Ctrl/Cmd+Shift+V  → clipboard paste, text only.
 *   5. Shift+Tab / Shift+Enter → encoded per the active provider
 *      profile.
 *   6. Anything else passes through to xterm's default handling.
 *
 * Tier 1 skipShell shortcuts (Ctrl+Shift+P, zoom, …) never reach this
 * function — the global capture listener claims them first. Tier 1
 * non-skipShell shortcuts (Ctrl+W, Ctrl+N, Ctrl+T) DO reach here when
 * the terminal is focused, fall through to `PASS`, and xterm forwards
 * the native bytes to the PTY so readline/tmux see them.
 */
export function resolveTerminalKey(ev: KeyboardEvent, providerId: string | null): KeyAction {
  if (ev.type !== 'keydown') return PASS

  // Match on `ev.code` (physical key) instead of `ev.key` (produced
  // character) so non-QWERTY layouts (Dvorak, AZERTY, Colemak) still
  // see the same Ctrl+C / Ctrl+V the user expects. `ev.key` would
  // change with the layout and silently break clipboard shortcuts.
  // Alt is excluded everywhere so Ctrl+Alt+V stays free for OS-level
  // bindings and matches the Tab/Enter checks below.
  const ctrlOrMeta = (ev.ctrlKey || ev.metaKey) && !ev.altKey

  if (ctrlOrMeta && ev.code === 'KeyC' && ev.shiftKey) {
    return BROWSER
  }

  if (ctrlOrMeta && ev.code === 'KeyC' && !ev.shiftKey) {
    return { kind: 'copy-or-sigint' }
  }

  if (ctrlOrMeta && ev.code === 'KeyV') {
    return ev.shiftKey ? { kind: 'paste-text-only' } : { kind: 'paste-text-or-image' }
  }

  const p = profileFor(providerId)

  if (ev.key === 'Tab' && ev.shiftKey && !ev.ctrlKey && !ev.altKey && !ev.metaKey) {
    if (p.shiftEncoding === 'legacy') return { kind: 'send-pty', bytes: '\x1b[Z' }
    if (p.shiftEncoding === 'kitty') return { kind: 'send-pty', bytes: '\x1b[9;2u' }
    return PASS
  }

  if (ev.key === 'Enter' && ev.shiftKey && !ev.ctrlKey && !ev.altKey && !ev.metaKey) {
    if (p.shiftEncoding === 'legacy') return { kind: 'send-pty', bytes: '\x1b\r' }
    if (p.shiftEncoding === 'kitty') return { kind: 'send-pty', bytes: '\x1b[13;2u' }
    return PASS
  }

  return PASS
}
