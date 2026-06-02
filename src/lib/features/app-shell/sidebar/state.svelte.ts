import { SvelteSet } from 'svelte/reactivity'
import { backend } from '$lib/api/backend'

export type SidebarView = 'git' | 'projects' | 'files' | 'issues'
export type GitSidebarTab = 'graph' | 'stashes' | 'branches'
export type ProjectSort = 'recent' | 'name'

let sidebarWidth = $state(280)
let sidebarCollapsed = $state(false)
let sidebarView = $state<SidebarView>('projects')
let gitSidebarTab = $state<GitSidebarTab>('graph')
let gitBranchesFocusRequests = $state<Map<string, number>>(new Map())

export function getSidebarWidth(): number {
  return sidebarWidth
}

export function setSidebarWidth(width: number) {
  sidebarWidth = Math.max(220, Math.min(520, width))
}

export function isSidebarCollapsed(): boolean {
  return sidebarCollapsed
}

export function setSidebarCollapsed(collapsed: boolean) {
  sidebarCollapsed = collapsed
}

export function toggleSidebar() {
  sidebarCollapsed = !sidebarCollapsed
}

export function getSidebarView(): SidebarView {
  return sidebarView
}

export function setSidebarView(view: SidebarView) {
  sidebarView = view
}

export function getGitSidebarTab(): GitSidebarTab {
  return gitSidebarTab
}

export function setGitSidebarTab(tab: GitSidebarTab) {
  gitSidebarTab = tab
}

export function requestGitBranchesFocus(projectId: string) {
  gitSidebarTab = 'branches'
  const next = new Map(gitBranchesFocusRequests)
  next.set(projectId, (next.get(projectId) ?? 0) + 1)
  gitBranchesFocusRequests = next
}

export function getGitBranchesFocusRequest(projectId: string): number {
  return gitBranchesFocusRequests.get(projectId) ?? 0
}

// PROJECTS NAV STATE //
// Which project nodes are expanded in the projects tree, and the sort
// order. Persisted to the app-state KV so they survive a reload; the
// projects sidebar hydrates them on first mount via loadProjectsNavState.
const PROJECTS_NAV_KEY = 'projects_nav'
const expandedProjectIds = new SvelteSet<string>()
let projectSort = $state<ProjectSort>('recent')

export function isProjectExpanded(projectId: string): boolean {
  return expandedProjectIds.has(projectId)
}

export function toggleProjectExpanded(projectId: string) {
  if (expandedProjectIds.has(projectId)) expandedProjectIds.delete(projectId)
  else expandedProjectIds.add(projectId)
  scheduleProjectsNavPersist()
}

export function getProjectSort(): ProjectSort {
  return projectSort
}

export function setProjectSort(sort: ProjectSort) {
  if (projectSort === sort) return
  projectSort = sort
  scheduleProjectsNavPersist()
}

let projectsNavTimer: ReturnType<typeof setTimeout> | null = null
function scheduleProjectsNavPersist() {
  if (projectsNavTimer) clearTimeout(projectsNavTimer)
  projectsNavTimer = setTimeout(() => {
    projectsNavTimer = null
    const payload = JSON.stringify({ expanded: [...expandedProjectIds], sort: projectSort })
    void backend.workspace.appStatePut(PROJECTS_NAV_KEY, payload).catch(() => {
      // Non-actionable preference write failure; keep the in-memory nav state.
    })
  }, 400)
}

let projectsNavLoaded = false
export async function loadProjectsNavState(): Promise<void> {
  if (projectsNavLoaded) return
  projectsNavLoaded = true
  try {
    const raw = await backend.workspace.appStateGet(PROJECTS_NAV_KEY)
    if (!raw) return
    const parsed = JSON.parse(raw) as { expanded?: unknown; sort?: unknown }
    if (Array.isArray(parsed.expanded)) {
      for (const id of parsed.expanded) if (typeof id === 'string') expandedProjectIds.add(id)
    }
    if (parsed.sort === 'recent' || parsed.sort === 'name') projectSort = parsed.sort
  } catch {
    // Non-actionable preference restore failure; defaults remain usable.
  }
}
