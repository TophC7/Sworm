import { backend } from '$lib/api/backend'
import type {
  Issue,
  IssueDetail,
  IssueEpic,
  IssueEpicUpdateInput,
  IssueListFilters,
  IssueSearchFilters,
  IssueUpdateInput
} from '$lib/types/backend'

let issuesByFolder = $state<Map<string, Issue[]>>(new Map())
let readyByFolder = $state<Map<string, Issue[]>>(new Map())
let epicsByFolder = $state<Map<string, IssueEpic[]>>(new Map())
let issueDetailsByFolder = $state<Map<string, Map<string, IssueDetail>>>(new Map())
let epicDetailsByFolder = $state<Map<string, Map<string, IssueEpic>>>(new Map())
let loadingByFolder = $state<Map<string, boolean>>(new Map())
let errorByFolder = $state<Map<string, string | null>>(new Map())

function setMapValue<K, V>(map: Map<K, V>, key: K, value: V): Map<K, V> {
  return new Map(map).set(key, value)
}

// Update one entry inside a per-folder nested map. Clones both levels
// so $state's reference equality treats it as a real change and
// downstream $derived caches re-run.
function setNested<V>(
  outer: Map<string, Map<string, V>>,
  folderPath: string,
  innerKey: string,
  value: V
): Map<string, Map<string, V>> {
  const next = new Map(outer)
  const inner = new Map(next.get(folderPath) ?? new Map())
  inner.set(innerKey, value)
  next.set(folderPath, inner)
  return next
}

function deleteNested<V>(
  outer: Map<string, Map<string, V>>,
  folderPath: string,
  innerKey: string
): Map<string, Map<string, V>> {
  const inner = outer.get(folderPath)
  if (!inner || !inner.has(innerKey)) return outer
  const next = new Map(outer)
  const cloned = new Map(inner)
  cloned.delete(innerKey)
  next.set(folderPath, cloned)
  return next
}

function messageFromError(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function setLoading(folderPath: string, loading: boolean) {
  loadingByFolder = setMapValue(loadingByFolder, folderPath, loading)
}

function setError(folderPath: string, message: string | null) {
  errorByFolder = setMapValue(errorByFolder, folderPath, message)
}

export function getIssues(folderPath: string): Issue[] {
  return issuesByFolder.get(folderPath) ?? []
}

export function getReadyIssues(folderPath: string): Issue[] {
  return readyByFolder.get(folderPath) ?? []
}

export function getIssueEpics(folderPath: string): IssueEpic[] {
  return epicsByFolder.get(folderPath) ?? []
}

// Per-id detail lookup. Each tab subscribes to its own slot so
// multiple tabs doesn't fight over a single shared
// cache entry.
export function getIssueDetail(folderPath: string, issueId: string): IssueDetail | null {
  return issueDetailsByFolder.get(folderPath)?.get(issueId) ?? null
}

export function getEpicDetail(folderPath: string, epicId: string): IssueEpic | null {
  return epicDetailsByFolder.get(folderPath)?.get(epicId) ?? null
}

export function isIssuesLoading(folderPath: string): boolean {
  return loadingByFolder.get(folderPath) ?? false
}

export function getIssuesError(folderPath: string): string | null {
  return errorByFolder.get(folderPath) ?? null
}

export async function loadIssues(folderPath: string, filters: IssueListFilters = {}) {
  setLoading(folderPath, true)
  setError(folderPath, null)
  try {
    const [issues, ready, epics] = await Promise.all([
      backend.issues.list(folderPath, filters),
      backend.issues.ready(folderPath, 20),
      backend.issues.epics.list(folderPath)
    ])
    issuesByFolder = setMapValue(issuesByFolder, folderPath, issues)
    readyByFolder = setMapValue(readyByFolder, folderPath, ready)
    epicsByFolder = setMapValue(epicsByFolder, folderPath, epics)
  } catch (error) {
    setError(folderPath, messageFromError(error))
  } finally {
    setLoading(folderPath, false)
  }
}

export async function searchIssues(
  folderPath: string,
  query: string,
  filters: IssueSearchFilters = {}
): Promise<Issue[]> {
  if (!query.trim()) return getIssues(folderPath)
  return backend.issues.search(folderPath, query.trim(), filters)
}

export async function openIssueDetail(folderPath: string, issueId: string): Promise<IssueDetail> {
  const detail = await backend.issues.get(folderPath, issueId)
  issueDetailsByFolder = setNested(issueDetailsByFolder, folderPath, issueId, detail)
  return detail
}

export function closeIssueDetail(folderPath: string, issueId: string) {
  issueDetailsByFolder = deleteNested(issueDetailsByFolder, folderPath, issueId)
}

export async function createIssue(
  folderPath: string,
  title: string,
  epicId: string,
  parentIssueId?: string | null
): Promise<Issue | null> {
  const trimmed = title.trim()
  if (!trimmed || (!epicId && !parentIssueId)) return null
  const issue = await backend.issues.create(folderPath, {
    title: trimmed,
    epicId: epicId || null,
    parentIssueId: parentIssueId ?? null,
    actor: 'human'
  })
  await loadIssues(folderPath)
  return issue
}

export async function createEpic(folderPath: string, title: string): Promise<IssueEpic | null> {
  const trimmed = title.trim()
  if (!trimmed) return null
  const epic = await backend.issues.epics.create(folderPath, {
    title: trimmed,
    actor: 'human'
  })
  await loadIssues(folderPath)
  return epic
}

export async function updateIssue(folderPath: string, issueId: string, patch: IssueUpdateInput): Promise<Issue> {
  const issue = await backend.issues.update(folderPath, issueId, {
    ...patch,
    actor: patch.actor ?? 'human'
  })
  await Promise.all([loadIssues(folderPath), openIssueDetail(folderPath, issueId)])
  return issue
}

export async function claimIssue(folderPath: string, issueId: string): Promise<Issue> {
  const gitUser = await backend.issues.currentGitUser(folderPath)
  return updateIssue(folderPath, issueId, {
    status: 'in_progress',
    assigneeKind: 'human',
    assigneeId: gitUser
  })
}

export async function openEpicDetail(folderPath: string, epicId: string): Promise<IssueEpic | null> {
  const epic = await backend.issues.epics.get(folderPath, epicId)
  if (epic) {
    epicDetailsByFolder = setNested(epicDetailsByFolder, folderPath, epicId, epic)
  }
  return epic
}

export function closeEpicDetail(folderPath: string, epicId: string) {
  epicDetailsByFolder = deleteNested(epicDetailsByFolder, folderPath, epicId)
}

export async function updateEpic(folderPath: string, epicId: string, patch: IssueEpicUpdateInput): Promise<IssueEpic> {
  const epic = await backend.issues.epics.update(folderPath, epicId, {
    ...patch,
    actor: patch.actor ?? 'human'
  })
  await Promise.all([loadIssues(folderPath), openEpicDetail(folderPath, epicId)])
  return epic
}

export async function deleteEpic(folderPath: string, epicId: string): Promise<void> {
  await backend.issues.epics.delete(folderPath, epicId)
  closeEpicDetail(folderPath, epicId)
  await loadIssues(folderPath)
}

export async function deleteIssue(folderPath: string, issueId: string): Promise<void> {
  await backend.issues.delete(folderPath, issueId)
  closeIssueDetail(folderPath, issueId)
  await loadIssues(folderPath)
}

export async function addIssueComment(folderPath: string, issueId: string, body: string): Promise<void> {
  const trimmed = body.trim()
  if (!trimmed) return
  await backend.issues.comments.add(folderPath, {
    issueId,
    author: 'human',
    body: trimmed,
    actor: 'human'
  })
  await openIssueDetail(folderPath, issueId)
}
