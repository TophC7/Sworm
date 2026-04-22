export type SidebarView = 'git' | 'sessions' | 'files'

let sidebarWidth = $state(280)
let sidebarCollapsed = $state(false)
let sidebarView = $state<SidebarView>('files')

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
