import type { Command, CommandGroup, FileCallbacks } from './types'
import { backend } from '$lib/api/backend'
import { getActiveProject, getProjects } from '$lib/stores/projects.svelte'
import { closeProject, getActiveProjectId, getOpenProjectIds, openProject } from '$lib/stores/workspace.svelte'
import { revealItemInDir } from '@tauri-apps/plugin-opener'
import { notify } from '$lib/stores/notifications.svelte'
import { getErrorMessage } from '$lib/utils/notifiedTask'
import { newEmptyFile, openProjectPicker } from './actions.svelte'

import {
  FilePlusIcon,
  FolderOpenIcon,
  FolderClockIcon,
  HomeIcon,
  SettingsIcon,
  SquareArrowOutUpRight,
  TerminalIcon,
  XIcon
} from '$lib/icons/lucideExports'

export function getFileCommands(callbacks: FileCallbacks): CommandGroup[] {
  const activeId = getActiveProjectId()
  const activeProject = getActiveProject()

  const commands: Command[] = [
    {
      id: 'new-file',
      label: 'New File',
      icon: FilePlusIcon,
      keywords: ['new', 'empty', 'untitled', 'file', 'create'],
      shortcut: 'Ctrl+N',
      onSelect: newEmptyFile
    },
    {
      id: 'open-project',
      label: 'Open Project',
      icon: FolderOpenIcon,
      keywords: ['new', 'add', 'folder', 'directory'],
      shortcut: 'Ctrl+O',
      onSelect: callbacks.onNewProject
    },
    {
      id: 'show-project-picker',
      label: 'Show Project Picker',
      icon: HomeIcon,
      keywords: ['home', 'picker', 'projects', 'empty', 'start'],
      shortcut: 'Ctrl+Shift+N',
      onSelect: openProjectPicker
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
    commands.push({
      id: 'open-in-external-terminal',
      label: 'Open in External Terminal',
      icon: TerminalIcon,
      keywords: ['terminal', 'shell', 'external', 'launch', 'kitty', 'alacritty', 'wezterm', 'gnome', 'konsole'],
      onSelect: () => {
        void backend.projects.openInTerminal(activeProject.path).catch((error) => {
          notify.error('Open in terminal failed', getErrorMessage(error))
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
