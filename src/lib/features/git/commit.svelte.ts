import { backend } from '$lib/api/backend'
import type { PostCommit } from '$lib/features/git/commitButton'
import type { GitSummary } from '$lib/types/backend'
import { SvelteMap } from 'svelte/reactivity'

export interface CommitState {
  draft: string
  postCommit: PostCommit
  signOff: boolean
  skipHooks: boolean
}

/** Never mutated; writers go through stateFor. */
const DEFAULT_STATE: CommitState = { draft: '', postCommit: 'none', signOff: false, skipHooks: false }
const states = new SvelteMap<string, CommitState>()
const prefsLoaded = new Set<string>()
const prefsTouched = new Set<string>()
const appliedTemplates = new Map<string, string>()

/** Folder teardown; post-commit and sign-off preferences reload from app state. */
export function releaseCommitFolder(folderPath: string): void {
  states.delete(folderPath)
  prefsLoaded.delete(folderPath)
  prefsTouched.delete(folderPath)
  appliedTemplates.delete(folderPath)
}

function stateFor(folderPath: string): CommitState {
  const existing = states.get(folderPath)
  if (existing) return existing
  const state: CommitState = $state({ ...DEFAULT_STATE })
  states.set(folderPath, state)
  return state
}

/** Safe during derivation: reading an unseen folder never creates state. */
export function getCommitState(folderPath: string): CommitState {
  return states.get(folderPath) ?? DEFAULT_STATE
}

export function setCommitDraft(folderPath: string, draft: string): void {
  stateFor(folderPath).draft = draft
}

/** Replace only an untouched template; drafts edited by the user remain theirs. */
export function syncCommitTemplate(folderPath: string, summary: GitSummary): void {
  const active = summary.message_template ?? summary.operation?.commit?.subject ?? null
  const state = getCommitState(folderPath)
  const previous = appliedTemplates.get(folderPath) ?? null
  if (active !== null) {
    if (state.draft === '' || state.draft === previous) {
      setCommitDraft(folderPath, active)
      appliedTemplates.set(folderPath, active)
    }
  } else {
    if (previous !== null && state.draft === previous) setCommitDraft(folderPath, '')
    appliedTemplates.delete(folderPath)
  }
}

/** Let Git retain its prepared message unless the user supplied a different one. */
export function getEditedCommitMessage(folderPath: string): string | null {
  const draft = getCommitState(folderPath).draft.trim()
  return draft && draft !== appliedTemplates.get(folderPath)?.trim() ? draft : null
}

export function setSkipHooks(folderPath: string, value: boolean): void {
  stateFor(folderPath).skipHooks = value
}

export function setPostCommit(folderPath: string, value: PostCommit): void {
  stateFor(folderPath).postCommit = value
  prefsTouched.add(folderPath)
  persist(folderPath)
}

export function setSignOff(folderPath: string, value: boolean): void {
  stateFor(folderPath).signOff = value
  prefsTouched.add(folderPath)
  persist(folderPath)
}

function persist(folderPath: string): void {
  const { postCommit, signOff } = getCommitState(folderPath)
  void backend.app.statePut(`gitCommit:${folderPath}`, JSON.stringify({ postCommit, signOff })).catch((e) => {
    console.error('Failed to persist commit prefs:', e)
  })
}

export async function loadCommitPrefs(folderPath: string): Promise<void> {
  if (prefsLoaded.has(folderPath)) return
  prefsLoaded.add(folderPath)
  let raw: string | null
  try {
    raw = await backend.app.stateGet(`gitCommit:${folderPath}`)
  } catch {
    return
  }
  // Released folders stay released; edits made during the load take precedence.
  if (!prefsLoaded.has(folderPath) || prefsTouched.has(folderPath) || raw === null) return
  try {
    const prefs: unknown = JSON.parse(raw)
    if (!prefs || typeof prefs !== 'object' || Array.isArray(prefs)) return
    const state = stateFor(folderPath)
    if (
      'postCommit' in prefs &&
      (prefs.postCommit === 'none' || prefs.postCommit === 'push' || prefs.postCommit === 'sync')
    ) {
      state.postCommit = prefs.postCommit
    }
    if ('signOff' in prefs && typeof prefs.signOff === 'boolean') state.signOff = prefs.signOff
  } catch {
    // Invalid persisted preferences leave the defaults intact.
  }
}
