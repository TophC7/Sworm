export type SidebarView = 'files' | 'git' | 'issues'
export type GitSidebarTab = 'graph' | 'stashes' | 'branches'

let sidebarWidth = $state(280)
let sidebarCollapsed = $state(false)
let sidebarView = $state<SidebarView>('files')
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

export function requestGitBranchesFocus(folderPath: string) {
  gitSidebarTab = 'branches'
  const next = new Map(gitBranchesFocusRequests)
  next.set(folderPath, (next.get(folderPath) ?? 0) + 1)
  gitBranchesFocusRequests = next
}

export function getGitBranchesFocusRequest(folderPath: string): number {
  return gitBranchesFocusRequests.get(folderPath) ?? 0
}
