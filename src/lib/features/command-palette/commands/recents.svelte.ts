// Recently-used command tracking for the Command Center.
//
// Stores the last N command IDs the user selected so the palette can
// surface them in a top "Recent" group. Persisted to localStorage
// because this is an app-level preference, not project-scoped state.

const STORAGE_KEY = 'sworm:recentCommands'
const MAX_RECENTS = 6

function load(): string[] {
  if (typeof localStorage === 'undefined') return []
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed = JSON.parse(raw)
    if (Array.isArray(parsed)) {
      return parsed.filter((v): v is string => typeof v === 'string').slice(0, MAX_RECENTS)
    }
  } catch {
    // Corrupt entry -- treat as empty so the palette still opens.
  }
  return []
}

function persist(ids: string[]): void {
  if (typeof localStorage === 'undefined') return
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(ids))
  } catch (err) {
    console.warn('Failed to persist recent commands:', err)
  }
}

let recents = $state<string[]>(load())

export function getRecentCommandIds(): string[] {
  return recents
}

/** Record a command as just-used. Moves it to the top, trims tail. */
export function recordRecentCommand(id: string): void {
  const next = [id, ...recents.filter((x) => x !== id)].slice(0, MAX_RECENTS)
  recents = next
  persist(next)
}

export function clearRecentCommands(): void {
  recents = []
  persist([])
}
