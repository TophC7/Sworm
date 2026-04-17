import type { Command, CommandGroup, FileCallbacks } from './types'
import { getActiveProject, getProjects } from '$lib/stores/projects.svelte'
import { closeProject, getActiveProjectId, getOpenProjectIds, openProject } from '$lib/stores/workspace.svelte'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import { notify } from '$lib/stores/notifications.svelte'
import { getErrorMessage } from '$lib/utils/notifiedTask'

import { FolderOpenIcon, FolderClockIcon, SettingsIcon, SquareArrowOutUpRight, XIcon } from '$lib/icons/lucideExports'

export function getFileCommands(callbacks: FileCallbacks): CommandGroup[] {
  const activeId = getActiveProjectId()
  const activeProject = getActiveProject()

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
      onSelect: () => void closeProject(activeId)
    })
  }

  if (activeProject) {
    commands.push({
      id: 'reveal-in-file-manager',
      label: 'Reveal in File Manager',
      icon: SquareArrowOutUpRight,
      keywords: ['open', 'folder', 'explorer', 'finder', 'nautilus', 'files'],
      onSelect: () => {
        void revealItemInDir(activeProject.path).catch((error) => {
          notify.error('Reveal in file manager failed', getErrorMessage(error))
        })
      }
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
