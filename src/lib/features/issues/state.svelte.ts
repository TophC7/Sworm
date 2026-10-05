import { SvelteMap } from 'svelte/reactivity'
import { backend } from '$lib/api/backend'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import { getErrorMessage } from '$lib/utils/client-error'
import type { Issue, IssueDetail, IssueEpic, IssueEpicUpdateInput, IssueUpdateInput } from '$lib/types/backend'

const lists = createFolderKeyedStore<{
  issues: Issue[]
  epics: IssueEpic[]
  loading: boolean
  error: string | null
}>()

type DetailSlot<T> = { value: T | null; error: string | null; sequence: number }
const issueDetails = new SvelteMap<string, DetailSlot<IssueDetail>>()
const epicDetails = new SvelteMap<string, DetailSlot<IssueEpic>>()
const refreshQueues = new Map<string, { again: boolean; promise: Promise<void> }>()

function detailKey(folderPath: string, id: string): string {
  return `${folderPath}\0${id}`
}

export function getIssues(folderPath: string): Issue[] {
  return lists.get(folderPath)?.issues ?? []
}

export function getIssueEpics(folderPath: string): IssueEpic[] {
  return lists.get(folderPath)?.epics ?? []
}

export function getIssueDetail(folderPath: string, issueId: string): DetailSlot<IssueDetail> | null {
  return issueDetails.get(detailKey(folderPath, issueId)) ?? null
}

export function getEpicDetail(folderPath: string, epicId: string): DetailSlot<IssueEpic> | null {
  return epicDetails.get(detailKey(folderPath, epicId)) ?? null
}

export function isIssuesLoading(folderPath: string): boolean {
  return lists.get(folderPath)?.loading ?? false
}

export function getIssuesError(folderPath: string): string | null {
  return lists.get(folderPath)?.error ?? null
}

async function refreshDetail<T>(
  details: SvelteMap<string, DetailSlot<T>>,
  key: string,
  slot: DetailSlot<T>,
  load: () => Promise<T>
): Promise<void> {
  const sequence = ++slot.sequence
  slot.error = null
  try {
    const value = await load()
    if (details.get(key) === slot && slot.sequence === sequence) slot.value = value
  } catch (error) {
    if (details.get(key) === slot && slot.sequence === sequence) slot.error = getErrorMessage(error)
  }
}

export function refreshIssuesForFolder(folderPath: string): Promise<void> {
  const prefix = `${folderPath}\0`
  const mountedIssues = () => [...issueDetails].filter(([key]) => key.startsWith(prefix))
  const mountedEpics = () => [...epicDetails].filter(([key]) => key.startsWith(prefix))
  if (!lists.has(folderPath) && !mountedIssues().length && !mountedEpics().length) return Promise.resolve()
  const existing = refreshQueues.get(folderPath)
  if (existing) {
    existing.again = true
    return existing.promise
  }

  const generation = lists.generation(folderPath)
  const queue = { again: false, promise: Promise.resolve() }
  const current = () => lists.generation(folderPath) === generation && refreshQueues.get(folderPath) === queue
  refreshQueues.set(folderPath, queue)
  queue.promise = (async () => {
    do {
      queue.again = false
      const reloadLists = async () => {
        if (!lists.has(folderPath)) return
        lists.patch(folderPath, { loading: true, error: null })
        try {
          const [issues, epics] = await Promise.all([
            backend.issues.list(folderPath),
            backend.issues.epics.list(folderPath)
          ])
          if (current()) lists.patch(folderPath, { issues, epics })
        } catch (error) {
          if (current()) lists.patch(folderPath, { error: getErrorMessage(error) })
        } finally {
          if (current()) lists.patch(folderPath, { loading: false })
        }
      }
      await Promise.all([
        reloadLists(),
        ...mountedIssues().map(([key, slot]) =>
          refreshDetail(issueDetails, key, slot, () => backend.issues.get(folderPath, key.slice(prefix.length)))
        ),
        ...mountedEpics().map(([key, slot]) =>
          refreshDetail(epicDetails, key, slot, () => backend.issues.epics.get(folderPath, key.slice(prefix.length)))
        )
      ])
    } while (current() && queue.again)
  })().finally(() => {
    if (refreshQueues.get(folderPath) === queue) refreshQueues.delete(folderPath)
  })
  return queue.promise
}

export function loadIssues(folderPath: string): Promise<void> {
  if (!lists.has(folderPath)) lists.set(folderPath, { issues: [], epics: [], loading: false, error: null })
  return refreshIssuesForFolder(folderPath)
}

export function watchIssueDetail(folderPath: string, issueId: string): () => void {
  const key = detailKey(folderPath, issueId)
  const slot = $state<DetailSlot<IssueDetail>>({ value: null, error: null, sequence: 0 })
  issueDetails.set(key, slot)
  void refreshIssuesForFolder(folderPath)
  return () => {
    if (issueDetails.get(key) === slot) issueDetails.delete(key)
  }
}

export function watchEpicDetail(folderPath: string, epicId: string): () => void {
  const key = detailKey(folderPath, epicId)
  const slot = $state<DetailSlot<IssueEpic>>({ value: null, error: null, sequence: 0 })
  epicDetails.set(key, slot)
  void refreshIssuesForFolder(folderPath)
  return () => {
    if (epicDetails.get(key) === slot) epicDetails.delete(key)
  }
}

export function reloadIssueDetail(folderPath: string, issueId: string): Promise<void> {
  if (!issueDetails.has(detailKey(folderPath, issueId))) return Promise.resolve()
  return refreshIssuesForFolder(folderPath)
}

export function reloadEpicDetail(folderPath: string, epicId: string): Promise<void> {
  if (!epicDetails.has(detailKey(folderPath, epicId))) return Promise.resolve()
  return refreshIssuesForFolder(folderPath)
}

export function releaseIssueFolder(folderPath: string): void {
  lists.delete(folderPath)
  refreshQueues.delete(folderPath)
  const prefix = `${folderPath}\0`
  for (const key of issueDetails.keys()) if (key.startsWith(prefix)) issueDetails.delete(key)
  for (const key of epicDetails.keys()) if (key.startsWith(prefix)) epicDetails.delete(key)
}

export async function createIssue(folderPath: string, title: string, epicId: string): Promise<Issue | null> {
  const trimmed = title.trim()
  if (!trimmed || !epicId) return null
  const issue = await backend.issues.create(folderPath, { title: trimmed, epicId, actor: 'human' })
  await refreshIssuesForFolder(folderPath)
  return issue
}

export async function createEpic(folderPath: string, title: string): Promise<IssueEpic | null> {
  const trimmed = title.trim()
  if (!trimmed) return null
  const epic = await backend.issues.epics.create(folderPath, { title: trimmed, actor: 'human' })
  await refreshIssuesForFolder(folderPath)
  return epic
}

export async function updateIssue(folderPath: string, issueId: string, patch: IssueUpdateInput): Promise<Issue> {
  const issue = await backend.issues.update(folderPath, issueId, { ...patch, actor: patch.actor ?? 'human' })
  await refreshIssuesForFolder(folderPath)
  return issue
}

export async function claimIssue(folderPath: string, issueId: string): Promise<Issue> {
  const gitUser = await backend.issues.currentGitUser(folderPath)
  return updateIssue(folderPath, issueId, { status: 'in_progress', assigneeKind: 'human', assigneeId: gitUser })
}

export async function updateEpic(folderPath: string, epicId: string, patch: IssueEpicUpdateInput): Promise<IssueEpic> {
  const epic = await backend.issues.epics.update(folderPath, epicId, { ...patch, actor: patch.actor ?? 'human' })
  await refreshIssuesForFolder(folderPath)
  return epic
}

export async function deleteEpic(folderPath: string, epicId: string): Promise<void> {
  await backend.issues.epics.delete(folderPath, epicId)
  await refreshIssuesForFolder(folderPath)
}

export async function deleteIssue(folderPath: string, issueId: string): Promise<void> {
  await backend.issues.delete(folderPath, issueId)
  await refreshIssuesForFolder(folderPath)
}

export async function addIssueComment(folderPath: string, issueId: string, body: string): Promise<void> {
  const trimmed = body.trim()
  if (!trimmed) return
  await backend.issues.comments.add(folderPath, { issueId, author: 'human', body: trimmed, actor: 'human' })
  await refreshIssuesForFolder(folderPath)
}
