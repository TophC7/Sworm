import type { Command, CommandGroup, FileCallbacks } from './types'
import { getProjects } from '$lib/stores/projects.svelte'
import { closeProject, getActiveProjectId, getOpenProjectIds, openProject } from '$lib/stores/workspace.svelte'

import { FolderOpenIcon, FolderClockIcon, SettingsIcon, XIcon } from '$lib/icons/lucideExports'

export function getFileCommands(callbacks: FileCallbacks): CommandGroup[] {
  const activeId = getActiveProjectId()

  const commands: Command[] = [
    {
      id: 'open-project',
      label: 'Open Project',
      icon: FolderOpenIcon,
      keywords: ['new', 'add', 'folder', 'directory'],
      shortcut: 'Ctrl+O',
      onSelect: callbacks.onNewProject
    },
    {
      id: 'settings',
      label: 'Settings',
      icon: SettingsIcon,
      keywords: ['preferences', 'config', 'options'],
      onSelect: callbacks.onSettings
    }
  ]

  if (activeId) {
    commands.push({
      id: 'close-project',
      label: 'Close Project',
      icon: XIcon,
      keywords: ['remove', 'close'],
      onSelect: () => closeProject(activeId)
    })
  }

  const groups: CommandGroup[] = [{ heading: 'File', commands }]

  const openIds = getOpenProjectIds()
  const recent = getProjects().filter((p) => !openIds.includes(p.id))

  if (recent.length > 0) {
    groups.push({
      heading: 'Recent Projects',
      commands: recent.map((p) => ({
        id: `recent-${p.id}`,
        label: p.name,
        subtitle: p.path,
        icon: FolderClockIcon,
        keywords: [p.name, p.path],
        onSelect: () => openProject(p.id)
      }))
    })
  }

  return groups
}
