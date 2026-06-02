// Shared app-menu model.
//
// Single source of truth for the titlebar menu's contents, so the
// horizontal menu bar (AppMenuBar) and the collapsed hamburger
// (TitleBarMenu) never drift. Reads reactive getters; call it from a
// `$derived` so the structure stays live.

import { getProjects } from '$lib/features/projects/state.svelte'
import { isSidebarCollapsed, toggleSidebar } from '$lib/features/app-shell/sidebar/state.svelte'
import { zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
import { closeActiveProject } from '$lib/features/app-actions/actions.svelte'
import { getActiveProjectId, getOpenProjectIds, openProject } from '$lib/features/workbench/state.svelte'

export interface MenuItem {
  kind: 'item'
  label: string
  onSelect: () => void
  disabled?: boolean
  title?: string
}

export interface MenuSubmenu {
  kind: 'submenu'
  label: string
  items: MenuItem[]
}

export interface MenuSeparator {
  kind: 'separator'
}

export type MenuEntry = MenuItem | MenuSubmenu | MenuSeparator

export interface MenuGroup {
  label: string
  entries: MenuEntry[]
}

export interface AppMenuHandlers {
  onNewProject: () => void
  onSettings: () => void
}

export function buildAppMenu(handlers: AppMenuHandlers): MenuGroup[] {
  const openIds = getOpenProjectIds()
  const openIdSet = new Set(openIds)
  const activeId = getActiveProjectId()
  const hasActive = activeId !== null
  const recent = getProjects().filter((p) => !openIdSet.has(p.id))

  const fileEntries: MenuEntry[] = [{ kind: 'item', label: 'Open Project', onSelect: handlers.onNewProject }]
  if (recent.length > 0) {
    fileEntries.push({
      kind: 'submenu',
      label: 'Open Recent',
      items: recent.map((p) => ({ kind: 'item', label: p.name, title: p.path, onSelect: () => openProject(p.id) }))
    })
  }
  fileEntries.push({
    kind: 'item',
    label: 'Close Project',
    disabled: !hasActive,
    onSelect: closeActiveProject
  })
  fileEntries.push({ kind: 'separator' })
  fileEntries.push({ kind: 'item', label: 'Settings…', onSelect: handlers.onSettings })

  const viewEntries: MenuEntry[] = [
    {
      kind: 'item',
      label: isSidebarCollapsed() ? 'Show Sidebar' : 'Hide Sidebar',
      disabled: !hasActive,
      onSelect: toggleSidebar
    },
    { kind: 'separator' },
    { kind: 'item', label: 'Zoom In', onSelect: zoomIn },
    { kind: 'item', label: 'Zoom Out', onSelect: zoomOut },
    { kind: 'item', label: 'Reset Zoom', onSelect: zoomReset }
  ]

  return [
    { label: 'File', entries: fileEntries },
    { label: 'View', entries: viewEntries }
  ]
}
