import type { Component } from 'svelte'
import {
  clearAllNotifications,
  getNotifications,
  isNotificationCenterOpen,
  setNotificationCenterOpen
} from '$lib/features/notifications/state.svelte'
import { allProviders, directOptions } from '$lib/features/sessions/providers/catalog'
import { getConnectedProviders } from '$lib/features/sessions/providers/state.svelte'
import { isSidebarCollapsed, toggleSidebar } from '$lib/features/app-shell/sidebar/state.svelte'
import { zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
import {
  isIndentRainbowEnabled,
  toggleIndentRainbow
} from '$lib/features/editor/renderers/monaco/text/indentRainbow.svelte'
import {
  closeActiveProject,
  closeActiveTab,
  createSessionWithSharedWorkspaceWarning,
  newEmptyFile,
  newTerminalSession,
  openActiveProjectInExternalTerminal,
  openFreshSession,
  openProjectDirectory,
  openProjectPicker,
  openSettings,
  reloadView,
  reopenTab,
  rerunLastProjectTask,
  revealActiveProjectInFileManager,
  showFiles,
  showTasks
} from '$lib/features/app-actions/actions.svelte'
import {
  fetchActiveProject,
  forcePushActiveProject,
  pullActiveProject,
  pushActiveProject,
  undoLastCommitActiveProject
} from '$lib/features/git/actions.svelte'
import { getGitSummary } from '$lib/features/git/state.svelte'
import { getTasksReactive } from '$lib/features/tasks/state.svelte'
import { getLastTaskId } from '$lib/features/tasks/service.svelte'
import { getActiveProjectId, hasClosedTabs } from '$lib/features/workbench/state.svelte'
import { openNotificationTool } from '$lib/features/workbench/surfaces/tool/service.svelte'
import {
  ArrowDownToLineIcon,
  ArrowUpFromLineIcon,
  BellIcon,
  FilePlusIcon,
  FolderOpenIcon,
  HomeIcon,
  PaintbrushIcon,
  PanelLeftIcon,
  Play,
  RefreshCwIcon,
  RotateCcwIcon,
  RotateCw,
  SearchIcon,
  SettingsIcon,
  ShieldAlertIcon,
  SquareArrowOutUpRight,
  TerminalIcon,
  Undo2Icon,
  XIcon,
  ZoomInIcon,
  ZoomOutIcon
} from '$lib/icons/lucideExports'

export type ShortcutCommandSource = 'app' | 'editor'
export type TerminalPolicy = 'defer' | 'skip-shell' | 'skip-shell-keeps-modals'

type Dynamic<T> = T | (() => T)

export interface AppCommandDefinition {
  id: string
  label: Dynamic<string>
  group: string
  keywords: Dynamic<string[]>
  icon?: Component
  iconSrc?: string
  defaultKeybindings?: string[]
  dangerous?: boolean
  terminalPolicy?: TerminalPolicy
  showInPalette?: boolean
  visible?: () => boolean
  subtitle?: () => string | undefined
  run: () => void | Promise<unknown>
}

export interface ShortcutCommandDefinition {
  id: string
  label: string
  group: string
  keywords: string[]
  source: ShortcutCommandSource
  dangerous?: boolean
  defaultKeybindings: string[]
  terminalPolicy?: TerminalPolicy
  run?: () => void | Promise<unknown>
}

function resolve<T>(value: Dynamic<T>): T {
  return typeof value === 'function' ? (value as () => T)() : value
}

function activeProjectVisible(): boolean {
  return getActiveProjectId() !== null
}

function connectedProviderIds(): Set<string> {
  return new Set(getConnectedProviders().map((provider) => provider.id))
}

function hasConnectedProvider(providerId: string): boolean {
  return connectedProviderIds().has(providerId)
}

function lastTaskLabel(): string | null {
  const projectId = getActiveProjectId()
  if (!projectId) return null
  const lastId = getLastTaskId(projectId)
  if (!lastId) return null
  const tasks = getTasksReactive(projectId)
  return tasks.find((task) => task.id === lastId)?.label ?? lastId
}

function appCommand(definition: AppCommandDefinition): AppCommandDefinition {
  return definition
}

export function getAppCommandDefinitions(): AppCommandDefinition[] {
  const providerCommands = allProviders.map((provider) =>
    appCommand({
      id: `new-session-${provider.id}`,
      label: `New ${provider.label} Session`,
      group: 'Sessions',
      iconSrc: provider.icon,
      keywords: ['new', 'session', 'agent', provider.label],
      visible: () => activeProjectVisible() && hasConnectedProvider(provider.id),
      run: () => createSessionWithSharedWorkspaceWarning(provider.id, provider.label)
    })
  )

  const freshIcon = directOptions.find((provider) => provider.id === 'fresh')?.icon ?? ''
  const terminalIcon = directOptions.find((provider) => provider.id === 'terminal')?.icon ?? ''

  return [
    appCommand({
      id: 'toggle-command-palette',
      label: 'Command Palette',
      group: 'General',
      keywords: ['command', 'palette', 'search'],
      defaultKeybindings: ['Ctrl+Shift+P'],
      terminalPolicy: 'skip-shell-keeps-modals',
      showInPalette: false,
      run: () => {}
    }),
    appCommand({
      id: 'new-file',
      label: 'New File',
      group: 'File',
      icon: FilePlusIcon,
      keywords: ['new', 'empty', 'untitled', 'file', 'create'],
      defaultKeybindings: ['Ctrl+N'],
      run: newEmptyFile
    }),
    appCommand({
      id: 'open-project',
      label: 'Open Project',
      group: 'File',
      icon: FolderOpenIcon,
      keywords: ['new', 'add', 'folder', 'directory'],
      defaultKeybindings: ['Ctrl+O'],
      run: openProjectDirectory
    }),
    appCommand({
      id: 'show-project-picker',
      label: 'Show Project Picker',
      group: 'File',
      icon: HomeIcon,
      keywords: ['home', 'picker', 'projects', 'empty', 'start'],
      defaultKeybindings: ['Ctrl+Shift+N'],
      terminalPolicy: 'skip-shell',
      run: openProjectPicker
    }),
    appCommand({
      id: 'settings',
      label: 'Settings',
      group: 'General',
      icon: SettingsIcon,
      keywords: ['preferences', 'config', 'options'],
      run: openSettings
    }),
    appCommand({
      id: 'close-project',
      label: 'Close Project',
      group: 'File',
      icon: XIcon,
      keywords: ['remove', 'close'],
      visible: activeProjectVisible,
      run: closeActiveProject
    }),
    appCommand({
      id: 'reveal-in-file-manager',
      label: 'Reveal in File Manager',
      group: 'File',
      icon: SquareArrowOutUpRight,
      keywords: ['open', 'folder', 'explorer', 'finder', 'nautilus', 'files'],
      visible: activeProjectVisible,
      run: revealActiveProjectInFileManager
    }),
    appCommand({
      id: 'open-in-external-terminal',
      label: 'Open in External Terminal',
      group: 'File',
      icon: TerminalIcon,
      keywords: ['terminal', 'shell', 'external', 'launch', 'kitty', 'alacritty', 'wezterm', 'gnome', 'konsole'],
      visible: activeProjectVisible,
      run: openActiveProjectInExternalTerminal
    }),
    ...providerCommands,
    appCommand({
      id: 'open-fresh',
      label: 'Open Fresh',
      group: 'Sessions',
      iconSrc: freshIcon,
      keywords: ['fresh', 'editor', 'text'],
      visible: () => activeProjectVisible() && hasConnectedProvider('fresh'),
      run: openFreshSession
    }),
    appCommand({
      id: 'new-terminal',
      label: 'New Terminal',
      group: 'Sessions',
      iconSrc: terminalIcon,
      keywords: ['terminal', 'shell', 'console'],
      defaultKeybindings: ['Ctrl+T'],
      visible: () => activeProjectVisible() && hasConnectedProvider('terminal'),
      run: newTerminalSession
    }),
    appCommand({
      id: 'files.show',
      label: 'Go to File',
      group: 'File',
      icon: SearchIcon,
      keywords: ['files', 'file', 'search', 'open', 'goto', 'quick', '/'],
      defaultKeybindings: ['Ctrl+P'],
      terminalPolicy: 'skip-shell-keeps-modals',
      visible: activeProjectVisible,
      run: showFiles
    }),
    appCommand({
      id: 'tasks.show',
      label: 'Show Tasks',
      group: 'Tasks',
      icon: Play,
      keywords: ['tasks', 'task', 'run', '!'],
      visible: activeProjectVisible,
      run: showTasks
    }),
    appCommand({
      id: 'tasks.rerun-last',
      label: 'Re-run Last Task',
      group: 'Tasks',
      icon: RotateCw,
      keywords: () => {
        const label = lastTaskLabel()
        return ['tasks', 'rerun', 'repeat', 'last', label ?? '']
      },
      visible: () => lastTaskLabel() !== null,
      subtitle: () => lastTaskLabel() ?? undefined,
      run: rerunLastProjectTask
    }),
    appCommand({
      id: 'toggle-sidebar',
      label: () => `${isSidebarCollapsed() ? 'Show' : 'Hide'} Sidebar`,
      group: 'View',
      icon: PanelLeftIcon,
      keywords: ['sidebar', 'panel', 'show', 'hide'],
      visible: activeProjectVisible,
      run: toggleSidebar
    }),
    appCommand({
      id: 'close-tab',
      label: 'Close Tab',
      group: 'View',
      icon: XIcon,
      keywords: ['close', 'tab', 'dismiss'],
      defaultKeybindings: ['Ctrl+W'],
      visible: activeProjectVisible,
      run: closeActiveTab
    }),
    appCommand({
      id: 'reopen-closed-tab',
      label: 'Reopen Closed Tab',
      group: 'View',
      icon: Undo2Icon,
      keywords: ['reopen', 'undo', 'restore', 'tab'],
      defaultKeybindings: ['Ctrl+Shift+T'],
      visible: () => {
        const projectId = getActiveProjectId()
        return projectId !== null && hasClosedTabs(projectId)
      },
      run: reopenTab
    }),
    appCommand({
      id: 'toggle-indent-rainbow',
      label: () => `${isIndentRainbowEnabled() ? 'Disable' : 'Enable'} Indent Rainbow`,
      group: 'View',
      icon: PaintbrushIcon,
      keywords: ['indent', 'rainbow', 'color', 'whitespace', 'toggle'],
      run: toggleIndentRainbow
    }),
    appCommand({
      id: 'reload-view',
      label: 'Reload View',
      group: 'View',
      icon: RefreshCwIcon,
      keywords: ['reload', 'refresh', 'hard', 'browser', 'force'],
      defaultKeybindings: ['Ctrl+Shift+R'],
      terminalPolicy: 'skip-shell',
      run: reloadView
    }),
    appCommand({
      id: 'zoom-in',
      label: 'Zoom In',
      group: 'View',
      icon: ZoomInIcon,
      keywords: ['zoom', 'larger', 'bigger', 'magnify'],
      defaultKeybindings: ['Ctrl+=', 'Ctrl++'],
      terminalPolicy: 'skip-shell-keeps-modals',
      run: zoomIn
    }),
    appCommand({
      id: 'zoom-out',
      label: 'Zoom Out',
      group: 'View',
      icon: ZoomOutIcon,
      keywords: ['zoom', 'smaller', 'shrink'],
      defaultKeybindings: ['Ctrl+-'],
      terminalPolicy: 'skip-shell-keeps-modals',
      run: zoomOut
    }),
    appCommand({
      id: 'zoom-reset',
      label: 'Reset Zoom',
      group: 'View',
      icon: RotateCcwIcon,
      keywords: ['zoom', 'reset', 'default', '100%'],
      defaultKeybindings: ['Ctrl+0'],
      terminalPolicy: 'skip-shell-keeps-modals',
      run: zoomReset
    }),
    appCommand({
      id: 'git-pull',
      label: 'Pull',
      group: 'Git',
      icon: ArrowDownToLineIcon,
      keywords: ['git', 'pull', 'download', 'sync'],
      visible: activeProjectVisible,
      run: pullActiveProject
    }),
    appCommand({
      id: 'git-push',
      label: 'Push',
      group: 'Git',
      icon: ArrowUpFromLineIcon,
      keywords: ['git', 'push', 'upload', 'sync'],
      visible: activeProjectVisible,
      run: pushActiveProject
    }),
    appCommand({
      id: 'git-fetch',
      label: 'Fetch',
      group: 'Git',
      icon: RefreshCwIcon,
      keywords: ['git', 'fetch', 'remote', 'update'],
      visible: activeProjectVisible,
      run: fetchActiveProject
    }),
    appCommand({
      id: 'git-force-push',
      label: 'Force Push (with lease)',
      group: 'Git',
      icon: ShieldAlertIcon,
      keywords: ['git', 'force', 'push', 'lease'],
      dangerous: true,
      visible: activeProjectVisible,
      run: forcePushActiveProject
    }),
    appCommand({
      id: 'git-undo-last-commit',
      label: 'Undo Last Commit',
      group: 'Git',
      icon: Undo2Icon,
      keywords: ['git', 'undo', 'reset', 'revert', 'commit'],
      dangerous: true,
      visible: () => {
        const projectId = getActiveProjectId()
        return projectId !== null && !!getGitSummary(projectId)?.branch
      },
      run: undoLastCommitActiveProject
    }),
    appCommand({
      id: 'open-notifications',
      label: () => (isNotificationCenterOpen() ? 'Hide Notifications' : 'Open Notifications'),
      group: 'Notifications',
      icon: BellIcon,
      keywords: ['notification', 'notifications', 'toast', 'alerts', 'open', 'show', 'hide', 'toggle'],
      run: () => setNotificationCenterOpen(!isNotificationCenterOpen())
    }),
    appCommand({
      id: 'dismiss-notifications',
      label: 'Dismiss Notifications',
      group: 'Notifications',
      icon: XIcon,
      keywords: ['notification', 'notifications', 'toast', 'alerts', 'dismiss', 'clear', 'remove'],
      visible: () => getNotifications().length > 0,
      run: clearAllNotifications
    }),
    appCommand({
      id: 'open-notification-tester',
      label: 'Open Notification Tester',
      group: 'Notifications',
      icon: BellIcon,
      keywords: ['notification', 'notifications', 'tester', 'preview', 'debug', 'demo', 'test'],
      visible: activeProjectVisible,
      run: () => {
        const projectId = getActiveProjectId()
        if (projectId) openNotificationTool(projectId)
      }
    })
  ]
}

export function getAppShortcutCommands(): ShortcutCommandDefinition[] {
  return getAppCommandDefinitions().map((definition) => ({
    id: definition.id,
    label: resolve(definition.label),
    group: definition.group,
    keywords: resolve(definition.keywords),
    source: 'app',
    dangerous: definition.dangerous,
    defaultKeybindings: definition.defaultKeybindings ?? [],
    terminalPolicy: definition.terminalPolicy,
    run: definition.run
  }))
}

export function getAppShortcutCommand(id: string): ShortcutCommandDefinition | null {
  return getAppShortcutCommands().find((command) => command.id === id) ?? null
}

export function getVisibleAppPaletteCommands(): AppCommandDefinition[] {
  return getAppCommandDefinitions().filter(
    (definition) => definition.showInPalette !== false && (definition.visible?.() ?? true)
  )
}

export function toPaletteCommand(definition: AppCommandDefinition) {
  return {
    id: definition.id,
    label: resolve(definition.label),
    subtitle: definition.subtitle?.(),
    icon: definition.icon,
    iconSrc: definition.iconSrc,
    keywords: resolve(definition.keywords),
    defaultKeybindings: definition.defaultKeybindings ?? [],
    onSelect: () => void definition.run()
  }
}

export function terminalPolicyOptions(policy: TerminalPolicy | undefined) {
  return {
    skipShell: policy === 'skip-shell' || policy === 'skip-shell-keeps-modals',
    keepsModals: policy === 'skip-shell-keeps-modals'
  }
}
