import { getContext, setContext } from 'svelte'

const SIDEBAR_CTX = Symbol('sidebar')

export type SidebarSide = 'left' | 'right'
export type CollapsibleMode = 'icon' | 'offcanvas' | 'none'

export interface SidebarState {
  readonly open: boolean
  readonly side: SidebarSide
  readonly collapsible: CollapsibleMode
  toggle: () => void
  setOpen: (value: boolean) => void
}

export function setSidebarContext(state: SidebarState) {
  setContext(SIDEBAR_CTX, state)
}

export function useSidebar(): SidebarState {
  return getContext<SidebarState>(SIDEBAR_CTX)
}
