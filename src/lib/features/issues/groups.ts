// Sort + group issues into the epic → top-level → sub-issue tree the
// sidebar renders. Counts include every issue under an epic (not just
// the filtered set) so the progress bar reflects real completion.

import type { Issue, IssueEpic } from '$lib/types/backend'
import { matchesIssue, type Term } from './query'

export type SortMode = 'priority:asc' | 'updated:desc' | 'created:desc' | 'created:asc' | 'title:asc'

export type EpicGroup = {
  epic: IssueEpic | null
  rows: { issue: Issue; subs: Issue[] }[]
  done: number
  active: number
  total: number
}

export function compareIssues(a: Issue, b: Issue, mode: SortMode): number {
  if (mode === 'priority:asc') {
    return a.priority - b.priority || Date.parse(b.updatedAt) - Date.parse(a.updatedAt)
  }
  if (mode === 'updated:desc') return Date.parse(b.updatedAt) - Date.parse(a.updatedAt)
  if (mode === 'created:desc') return Date.parse(b.createdAt) - Date.parse(a.createdAt)
  if (mode === 'created:asc') return Date.parse(a.createdAt) - Date.parse(b.createdAt)
  return a.title.localeCompare(b.title)
}

function counts(list: Issue[]) {
  let done = 0
  let active = 0
  for (const i of list) {
    if (i.status === 'completed' || i.status === 'wont_fix') done += 1
    else if (i.status === 'in_progress') active += 1
  }
  return { done, active, total: list.length }
}

export function groupIssuesByEpic(issues: Issue[], epics: IssueEpic[], terms: Term[], sortMode: SortMode): EpicGroup[] {
  const filtered = issues.filter((i) => matchesIssue(i, terms))

  const byParent = new Map<string, Issue[]>()
  const tops: Issue[] = []
  for (const i of filtered) {
    if (i.parentIssueId) {
      const list = byParent.get(i.parentIssueId) ?? []
      list.push(i)
      byParent.set(i.parentIssueId, list)
    } else {
      tops.push(i)
    }
  }
  const cmp = (a: Issue, b: Issue) => compareIssues(a, b, sortMode)
  for (const list of byParent.values()) list.sort(cmp)
  tops.sort(cmp)

  const allByEpic = new Map<string, Issue[]>()
  const allOrphans: Issue[] = []
  for (const i of issues) {
    if (!i.epicId) {
      allOrphans.push(i)
      continue
    }
    const list = allByEpic.get(i.epicId) ?? []
    list.push(i)
    allByEpic.set(i.epicId, list)
  }

  const out: EpicGroup[] = []
  for (const e of epics) {
    const epicTops = tops.filter((i) => i.epicId === e.id)
    const rows = epicTops.map((issue) => ({
      issue,
      subs: byParent.get(issue.id) ?? []
    }))
    out.push({ epic: e, rows, ...counts(allByEpic.get(e.id) ?? []) })
  }

  // Schema allows epicId=null. Render orphans in a catch-all group so
  // they don't silently disappear from the tree.
  const orphanTops = tops.filter((i) => !i.epicId)
  if (orphanTops.length > 0 || allOrphans.length > 0) {
    out.push({
      epic: null,
      rows: orphanTops.map((issue) => ({
        issue,
        subs: byParent.get(issue.id) ?? []
      })),
      ...counts(allOrphans)
    })
  }

  return out
}
