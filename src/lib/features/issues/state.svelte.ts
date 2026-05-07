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

let issuesByProject = $state<Map<string, Issue[]>>(new Map())
let readyByProject = $state<Map<string, Issue[]>>(new Map())
let epicsByProject = $state<Map<string, IssueEpic[]>>(new Map())
let issueDetailsByProject = $state<Map<string, Map<string, IssueDetail>>>(new Map())
let epicDetailsByProject = $state<Map<string, Map<string, IssueEpic>>>(new Map())
let loadingByProject = $state<Map<string, boolean>>(new Map())
let errorByProject = $state<Map<string, string | null>>(new Map())

function setMapValue<K, V>(map: Map<K, V>, key: K, value: V): Map<K, V> {
  return new Map(map).set(key, value)
}

// Update one entry inside a per-project nested map. Clones both levels
// so $state's reference equality treats it as a real change and
// downstream $derived caches re-run.
function setNested<V>(
  outer: Map<string, Map<string, V>>,
  projectId: string,
  innerKey: string,
  value: V
): Map<string, Map<string, V>> {
  const next = new Map(outer)
  const inner = new Map(next.get(projectId) ?? new Map())
  inner.set(innerKey, value)
  next.set(projectId, inner)
  return next
}

function deleteNested<V>(
  outer: Map<string, Map<string, V>>,
  projectId: string,
  innerKey: string
): Map<string, Map<string, V>> {
  const inner = outer.get(projectId)
  if (!inner || !inner.has(innerKey)) return outer
  const next = new Map(outer)
  const cloned = new Map(inner)
  cloned.delete(innerKey)
  next.set(projectId, cloned)
  return next
}

function messageFromError(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function setLoading(projectId: string, loading: boolean) {
  loadingByProject = setMapValue(loadingByProject, projectId, loading)
}

function setError(projectId: string, message: string | null) {
  errorByProject = setMapValue(errorByProject, projectId, message)
}

export function getIssues(projectId: string): Issue[] {
  return issuesByProject.get(projectId) ?? []
}

export function getReadyIssues(projectId: string): Issue[] {
  return readyByProject.get(projectId) ?? []
}

export function getIssueEpics(projectId: string): IssueEpic[] {
  return epicsByProject.get(projectId) ?? []
}

// Per-id detail lookup. Each tab subscribes to its own slot so
// multi-tab / split-pane editing doesn't fight over a single shared
// cache entry.
export function getIssueDetail(projectId: string, issueId: string): IssueDetail | null {
  return issueDetailsByProject.get(projectId)?.get(issueId) ?? null
}

export function getEpicDetail(projectId: string, epicId: string): IssueEpic | null {
  return epicDetailsByProject.get(projectId)?.get(epicId) ?? null
}

export function isIssuesLoading(projectId: string): boolean {
  return loadingByProject.get(projectId) ?? false
}

export function getIssuesError(projectId: string): string | null {
  return errorByProject.get(projectId) ?? null
}

export async function loadIssues(projectId: string, filters: IssueListFilters = {}) {
  setLoading(projectId, true)
  setError(projectId, null)
  try {
    const [issues, ready, epics] = await Promise.all([
      backend.issues.list(projectId, filters),
      backend.issues.ready(projectId, 20),
      backend.issues.epics.list(projectId)
    ])
    issuesByProject = setMapValue(issuesByProject, projectId, issues)
    readyByProject = setMapValue(readyByProject, projectId, ready)
    epicsByProject = setMapValue(epicsByProject, projectId, epics)
  } catch (error) {
    setError(projectId, messageFromError(error))
  } finally {
    setLoading(projectId, false)
  }
}

export async function searchIssues(
  projectId: string,
  query: string,
  filters: IssueSearchFilters = {}
): Promise<Issue[]> {
  if (!query.trim()) return getIssues(projectId)
  return backend.issues.search(projectId, query.trim(), filters)
}

export async function openIssueDetail(projectId: string, issueId: string): Promise<IssueDetail> {
  const detail = await backend.issues.get(projectId, issueId)
  issueDetailsByProject = setNested(issueDetailsByProject, projectId, issueId, detail)
  return detail
}

export function closeIssueDetail(projectId: string, issueId: string) {
  issueDetailsByProject = deleteNested(issueDetailsByProject, projectId, issueId)
}

export async function createIssue(
  projectId: string,
  title: string,
  epicId: string,
  parentIssueId?: string | null
): Promise<Issue | null> {
  const trimmed = title.trim()
  if (!trimmed || (!epicId && !parentIssueId)) return null
  const issue = await backend.issues.create(projectId, {
    title: trimmed,
    epicId: epicId || null,
    parentIssueId: parentIssueId ?? null,
    actor: 'human'
  })
  await loadIssues(projectId)
  return issue
}

export async function createEpic(projectId: string, title: string): Promise<IssueEpic | null> {
  const trimmed = title.trim()
  if (!trimmed) return null
  const epic = await backend.issues.epics.create(projectId, {
    title: trimmed,
    actor: 'human'
  })
  await loadIssues(projectId)
  return epic
}

export async function updateIssue(projectId: string, issueId: string, patch: IssueUpdateInput): Promise<Issue> {
  const issue = await backend.issues.update(projectId, issueId, {
    ...patch,
    actor: patch.actor ?? 'human'
  })
  await Promise.all([loadIssues(projectId), openIssueDetail(projectId, issueId)])
  return issue
}

export async function claimIssue(projectId: string, issueId: string): Promise<Issue> {
  const gitUser = await backend.issues.currentGitUser(projectId)
  return updateIssue(projectId, issueId, {
    status: 'in_progress',
    assigneeKind: 'human',
    assigneeId: gitUser
  })
}

export async function openEpicDetail(projectId: string, epicId: string): Promise<IssueEpic | null> {
  const epic = await backend.issues.epics.get(projectId, epicId)
  if (epic) {
    epicDetailsByProject = setNested(epicDetailsByProject, projectId, epicId, epic)
  }
  return epic
}

export function closeEpicDetail(projectId: string, epicId: string) {
  epicDetailsByProject = deleteNested(epicDetailsByProject, projectId, epicId)
}

export async function updateEpic(projectId: string, epicId: string, patch: IssueEpicUpdateInput): Promise<IssueEpic> {
  const epic = await backend.issues.epics.update(projectId, epicId, {
    ...patch,
    actor: patch.actor ?? 'human'
  })
  await Promise.all([loadIssues(projectId), openEpicDetail(projectId, epicId)])
  return epic
}

export async function deleteEpic(projectId: string, epicId: string): Promise<void> {
  await backend.issues.epics.delete(projectId, epicId)
  closeEpicDetail(projectId, epicId)
  await loadIssues(projectId)
}

export async function deleteIssue(projectId: string, issueId: string): Promise<void> {
  await backend.issues.delete(projectId, issueId)
  closeIssueDetail(projectId, issueId)
  await loadIssues(projectId)
}

export async function addIssueComment(projectId: string, issueId: string, body: string): Promise<void> {
  const trimmed = body.trim()
  if (!trimmed) return
  await backend.issues.comments.add(projectId, {
    issueId,
    author: 'human',
    body: trimmed,
    actor: 'human'
  })
  await openIssueDetail(projectId, issueId)
}
