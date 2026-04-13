/**
 * PTY output activity classifier.
 *
 * Analyzes raw terminal output chunks to determine whether an agent CLI
 * is busy (thinking/executing) or idle (waiting for user input).
 * Provider-specific patterns ported from emdash's activityClassifier.
 */

export type ActivitySignal = 'busy' | 'idle' | 'neutral'

const MAX_CHUNK_CHARS = 8_192

// Strip ANSI escape sequences for reliable text matching.
// Covers CSI sequences, OSC sequences, and simple escapes.
const ANSI_RE = /\x1b(?:\[[0-?]*[ -/]*[@-~]|\][^\x07\x1b]*(?:\x07|\x1b\\)|[()#][A-Z0-9]|[=><=A-Z])/g

function stripAnsi(text: string): string {
  return text.replace(ANSI_RE, '').replace(/\r/g, '')
}

/**
 * Bound analysis to the most recent portion of large chunks.
 * Newest output carries the strongest busy/idle signal.
 */
export function sampleChunk(chunk: string): string {
  if (!chunk) return ''
  return chunk.length <= MAX_CHUNK_CHARS ? chunk : chunk.slice(-MAX_CHUNK_CHARS)
}

/**
 * Classify a PTY output chunk as busy, idle, or neutral.
 *
 * Provider ID should match the session's provider_id field
 * (e.g. 'claude_code', 'codex', 'copilot').
 */
export function classifyActivity(providerId: string | null | undefined, chunk: string): ActivitySignal {
  const text = stripAnsi(sampleChunk(chunk))
  if (!text) return 'neutral'

  const p = (providerId || '').toLowerCase()

  // -- Claude Code --
  if (p === 'claude_code' || p === 'claude') {
    if (/esc\s*to\s*interrupt/i.test(text)) return 'busy'
    if (/wrangling|crafting|thinking|reasoning|analyzing|planning|reading|scanning|applying/i.test(text)) return 'busy'
    if (/Ready|Awaiting|Next command|Use \/login/i.test(text)) return 'idle'
  }

  // -- Codex --
  if (p === 'codex') {
    if (/Esc to interrupt/i.test(text)) return 'busy'
    if (/\(\s*(?:\d+\s*m\s*)?\d+\s*s\s*•\s*Esc to interrupt\s*\)/i.test(text)) return 'busy'
    if (/Responding to\b/i.test(text)) return 'busy'
    if (
      /Executing|Running|Thinking|Working|Analyzing|Identifying|Inspecting|Summarizing|Refactoring|Applying|Updating|Generating|Scanning|Parsing|Checking/i.test(
        text
      )
    )
      return 'busy'
    if (/Ready|Awaiting input|Press Enter/i.test(text)) return 'idle'
    if (/\b\/(status|approvals|model)\b/i.test(text)) return 'idle'
    if (/send\s+\S*\s*newline|transcript|quit/i.test(text)) return 'idle'
  }

  // -- GitHub Copilot --
  if (p === 'copilot') {
    if (/Thinking|Working|Generating/i.test(text)) return 'busy'
    if (
      /Ready|Press Enter|Next step/i.test(text) ||
      /Do you want to/i.test(text) ||
      /Confirm with number keys/i.test(text) ||
      /approve all file operations/i.test(text) ||
      /Yes, and approve/i.test(text)
    )
      return 'idle'
  }

  // -- Crush --
  if (p === 'crush') {
    if (/Processing|Generating|Running|Executing/i.test(text)) return 'busy'
    if (/Thinking|Working|Analyzing|Building/i.test(text)) return 'busy'
    if (/[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/.test(text)) return 'busy'
    if (/Ready|Awaiting|Press Enter/i.test(text)) return 'idle'
    if (/crush\s*>/i.test(text)) return 'idle'
    if (/What.*\?|Choose|Select/i.test(text)) return 'idle'
  }

  // -- Gemini CLI --
  if (p === 'gemini') {
    if (/esc\s*to\s*cancel/i.test(text)) return 'busy'
    if (/Thinking\.{0,3}/i.test(text)) return 'busy'
    if (/Running|Working|Executing|Generating|Applying|Planning|Analyzing/i.test(text)) return 'busy'
    if (/Ready|Awaiting|Press Enter/i.test(text)) return 'idle'
  }

  // -- OpenCode --
  if (p === 'opencode') {
    if (/Thinking\.{0,3}/i.test(text)) return 'busy'
    if (/waiting\s+for\s+response/i.test(text)) return 'busy'
    if (/esc\s*to\s*cancel/i.test(text)) return 'busy'
    if (/[⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏]/.test(text)) return 'busy'
    if (/Ready|Awaiting|Press Enter|Next command|Type your message/i.test(text)) return 'idle'
  }

  // -- Generic fallback (covers unlisted providers) --
  if (/esc\s*to\s*(cancel|interrupt)/i.test(text)) return 'busy'
  if (/(^|\b)(Generating|Working|Executing|Running|Applying|Thinking)(\b|\.)/i.test(text)) return 'busy'
  if (/Add a follow-up|Ready|Awaiting|Press Enter|Next command/i.test(text)) return 'idle'

  return 'neutral'
}
