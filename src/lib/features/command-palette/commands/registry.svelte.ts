import type { Component } from 'svelte'
import { platform, type PlatformCapabilities } from '$lib/platform'
import {
  clearAllNotifications,
  getNotifications,
  isNotificationCenterOpen,
  setNotificationCenterOpen
} from '$lib/features/notifications/state.svelte'
import { allProviders } from '$lib/features/sessions/providers/catalog'
import { getConnectedProviders } from '$lib/features/sessions/providers/state.svelte'
import { isSidebarCollapsed, toggleSidebar } from '$lib/features/app-shell/sidebar/state.svelte'
import { zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
import { browseServer, localHostLabel, openBrowser } from '$lib/features/browser/state.svelte'
import { getServers } from '$lib/features/browser/places.svelte'
import { openRemoteManager } from '$lib/features/remotes/state.svelte'
import { getGroups, getTabGroup, retryGroup } from '$lib/features/workbench/groups.svelte'
import { openCommandPaletteWithSearch } from '$lib/features/command-palette/state.svelte'
import {
  isIndentRainbowEnabled,
  toggleIndentRainbow
} from '$lib/features/editor/renderers/monaco/text/indentRainbow.svelte'
import { splitRemotePath } from '$lib/utils/paths'
import {
  closeActiveTab,
  closeCurrentWorkbench,
  createSession,
  newEmptyFile,
  newTerminalSession,
  newWindow,
  openActiveFolderInExternalTerminal,
  openFolderPicker,
  openFolderSettingsFile,
  openGlobalSettingsFile,
  openSettings,
  reloadView,
  reopenTab,
  rerunLastFolderTask,
  revealActiveFolderInFileManager,
  showFiles,
  showTasks,
  takeOverServerWorkbench,
  moveServerWorkbenchToNewWindow,
  removeServerWorkbenchFromWindow
} from '$lib/features/app-actions/actions.svelte'
import {
  fetchActiveFolder,
  forcePushActiveFolder,
  pullActiveFolder,
  pushActiveFolder,
  undoLastCommitActiveFolder
} from '$lib/features/git/actions.svelte'
import { getGitSummary } from '$lib/features/git/state.svelte'
import { getTasksReactive } from '$lib/features/tasks/state.svelte'
import { getLastTaskId } from '$lib/features/tasks/service.svelte'
import {
  addNotificationToolTab,
  getActiveFolderPath,
  getActiveTab,
  hasClosedTabs
} from '$lib/features/workbench/state.svelte'
import {
  ArrowDownToLineIcon,
  ArrowUpFromLineIcon,
  BellIcon,
  CompassIcon,
  FilePlusCornerIcon,
  FolderOpenIcon,
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
  ServerIcon,
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
  capability?: keyof PlatformCapabilities
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

function activeFolderVisible(): boolean {
  return getActiveFolderPath() !== null
}

function hasConnectedProvider(providerId: string): boolean {
  return getConnectedProviders(getActiveFolderPath()).some((provider) => provider.id === providerId)
}

function lastTaskLabel(): string | null {
  const folderPath = getActiveFolderPath()
  if (!folderPath) return null
  const lastId = getLastTaskId(folderPath)
  if (!lastId) return null
  const tasks = getTasksReactive(folderPath)
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
      visible: () => activeFolderVisible() && hasConnectedProvider(provider.id),
      run: () => createSession(provider.id, provider.label)
    })
  )

  const serverCommands = platform.native
    ? getServers().map(({ name }) =>
        appCommand({
          id: `browse-server-${name}`,
          label: `Browse ${name}`,
          group: 'Places',
          icon: ServerIcon,
          keywords: ['browse', 'remote', 'server', name],
          run: () => browseServer(name)
        })
      )
    : []
  const activeTab = getActiveTab()
  const activeGroup = activeTab ? getTabGroup(activeTab) : null
  const groupCommands = getGroups().flatMap((group) => [
    appCommand({
      id: `workbench-take-over-${group.server}`,
      label: `Take Over ${group.server} Workbench`,
      group: 'Workbenches',
      icon: ServerIcon,
      keywords: ['take', 'over', 'workbench', 'remote', group.server],
      visible: () => group.state === 'busy' || group.state === 'revoked',
      run: () => takeOverServerWorkbench(group.server)
    }),
    appCommand({
      id: `workbench-reconnect-${group.server}`,
      label: `Reconnect ${group.server} Workbench`,
      group: 'Workbenches',
      icon: RefreshCwIcon,
      keywords: ['reconnect', 'retry', 'offline', 'workbench', group.server],
      visible: () => group.state === 'offline',
      run: () => retryGroup(group.server)
    }),
    appCommand({
      id: `workbench-move-to-new-window-${group.server}`,
      label: `Move ${group.server} Workbench to New Window`,
      group: 'Workbenches',
      keywords: ['move', 'new', 'window', 'workbench', group.server],
      visible: () => group.state === 'active' && activeGroup?.server === group.server,
      run: () => moveServerWorkbenchToNewWindow(group.server)
    }),
    appCommand({
      id: `workbench-remove-from-window-${group.server}`,
      label: `Remove ${group.server} Workbench from Window`,
      group: 'Workbenches',
      icon: XIcon,
      keywords: ['remove', 'window', 'workbench', group.server],
      visible: () => group.state !== 'active',
      run: () => removeServerWorkbenchFromWindow(group.server)
    })
  ])
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
      icon: FilePlusCornerIcon,
      keywords: ['new', 'empty', 'untitled', 'file', 'create'],
      capability: 'saveAsDialog',
      defaultKeybindings: ['Ctrl+N'],
      visible: activeFolderVisible,
      run: newEmptyFile
    }),
    appCommand({
      id: 'new-window',
      label: 'New Window',
      group: 'File',
      keywords: ['new', 'window', 'workbench'],
      capability: 'nativeWindowControls',
      defaultKeybindings: ['Ctrl+Shift+N'],
      terminalPolicy: 'skip-shell',
      run: newWindow
    }),
    appCommand({
      id: 'open-folder',
      label: 'Open Folder',
      group: 'File',
      icon: FolderOpenIcon,
      keywords: ['open', 'folder', 'directory', 'places', 'workbench', 'remote', 'server', 'recent', 'browse'],
      defaultKeybindings: ['Ctrl+O'],
      terminalPolicy: 'skip-shell-keeps-modals',
      run: () => openBrowser()
    }),
    appCommand({
      id: 'open-folder-native',
      label: 'Open Folder with System Dialog…',
      group: 'File',
      icon: FolderOpenIcon,
      keywords: ['open', 'folder', 'directory', 'native', 'system', 'dialog'],
      capability: 'nativeDirectoryPicker',
      run: openFolderPicker
    }),
    appCommand({
      id: 'close-workbench',
      label: 'Close Workbench',
      group: 'File',
      icon: XIcon,
      keywords: ['close', 'workbench', 'stop'],
      dangerous: true,
      capability: 'durableWorkbenches',
      run: closeCurrentWorkbench
    }),
    appCommand({
      id: 'places.show',
      label: 'Go to Place…',
      group: 'Places',
      icon: CompassIcon,
      keywords: ['places', 'workbench', 'folder', 'remote', 'server', 'recent', 'browse', '@'],
      run: () => openCommandPaletteWithSearch('@ ')
    }),
    appCommand({
      id: 'browse-local',
      label: `Browse ${localHostLabel()}`,
      group: 'Places',
      icon: CompassIcon,
      keywords: ['browse', 'local', 'folder', localHostLabel()],
      run: () => browseServer(null)
    }),
    ...serverCommands,
    ...groupCommands,
    appCommand({
      id: 'manage-servers',
      label: 'Manage Servers…',
      group: 'Places',
      icon: ServerIcon,
      keywords: ['manage', 'pair', 'remote', 'server'],
      visible: () => platform.capabilities.remoteHosts,
      run: openRemoteManager
    }),
    appCommand({
      id: 'settings',
      label: 'Settings',
      group: 'General',
      icon: SettingsIcon,
      keywords: ['preferences', 'config', 'options'],
      defaultKeybindings: ['Ctrl+,'],
      terminalPolicy: 'skip-shell',
      run: openSettings
    }),
    appCommand({
      id: 'open-global-settings',
      label: 'Open Global Settings',
      group: 'General',
      icon: SettingsIcon,
      keywords: ['settings', 'preferences', 'jsonc', 'global', 'user'],
      capability: 'externalFileOpen',
      run: openGlobalSettingsFile
    }),
    appCommand({
      id: 'open-folder-settings',
      label: 'Open Folder Settings',
      group: 'General',
      icon: SettingsIcon,
      keywords: ['settings', 'preferences', 'jsonc', 'folder', 'sworm'],
      visible: activeFolderVisible,
      run: openFolderSettingsFile
    }),
    appCommand({
      id: 'reveal-in-file-manager',
      label: 'Reveal in File Manager',
      group: 'File',
      icon: SquareArrowOutUpRight,
      keywords: ['open', 'folder', 'explorer', 'finder', 'nautilus', 'files'],
      capability: 'revealInFileManager',
      visible: () => activeFolderVisible() && !splitRemotePath(getActiveFolderPath() ?? ''),
      run: revealActiveFolderInFileManager
    }),
    appCommand({
      id: 'open-in-external-terminal',
      label: 'Open in External Terminal',
      group: 'File',
      icon: TerminalIcon,
      keywords: ['terminal', 'shell', 'external', 'launch', 'kitty', 'alacritty', 'wezterm', 'gnome', 'konsole'],
      capability: 'openInTerminal',
      visible: activeFolderVisible,
      run: openActiveFolderInExternalTerminal
    }),
    ...providerCommands,
    appCommand({
      id: 'new-terminal',
      label: 'New Terminal',
      group: 'Sessions',
      icon: TerminalIcon,
      keywords: ['terminal', 'shell', 'console'],
      // Web: Ctrl+T/W/Shift+T belong to browser tab management.
      defaultKeybindings: platform.native ? ['Ctrl+T'] : [],
      visible: activeFolderVisible,
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
      visible: activeFolderVisible,
      run: showFiles
    }),
    appCommand({
      id: 'tasks.show',
      label: 'Show Tasks',
      group: 'Tasks',
      icon: Play,
      keywords: ['tasks', 'task', 'run', '!'],
      visible: activeFolderVisible,
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
      run: rerunLastFolderTask
    }),
    appCommand({
      id: 'toggle-sidebar',
      label: () => `${isSidebarCollapsed() ? 'Show' : 'Hide'} Sidebar`,
      group: 'View',
      icon: PanelLeftIcon,
      keywords: ['sidebar', 'panel', 'show', 'hide'],
      visible: activeFolderVisible,
      run: toggleSidebar
    }),
    appCommand({
      id: 'close-tab',
      label: 'Close Tab',
      group: 'View',
      icon: XIcon,
      keywords: ['close', 'tab', 'dismiss'],
      defaultKeybindings: platform.native ? ['Ctrl+W'] : [],
      visible: activeFolderVisible,
      run: closeActiveTab
    }),
    appCommand({
      id: 'reopen-closed-tab',
      label: 'Reopen Closed Tab',
      group: 'View',
      icon: Undo2Icon,
      keywords: ['reopen', 'undo', 'restore', 'tab'],
      defaultKeybindings: platform.native ? ['Ctrl+Shift+T'] : [],
      visible: hasClosedTabs,
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
      capability: 'zoom',
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
      capability: 'zoom',
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
      capability: 'zoom',
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
      visible: activeFolderVisible,
      run: pullActiveFolder
    }),
    appCommand({
      id: 'git-push',
      label: 'Push',
      group: 'Git',
      icon: ArrowUpFromLineIcon,
      keywords: ['git', 'push', 'upload', 'sync'],
      visible: activeFolderVisible,
      run: pushActiveFolder
    }),
    appCommand({
      id: 'git-fetch',
      label: 'Fetch',
      group: 'Git',
      icon: RefreshCwIcon,
      keywords: ['git', 'fetch', 'remote', 'update'],
      visible: activeFolderVisible,
      run: fetchActiveFolder
    }),
    appCommand({
      id: 'git-force-push',
      label: 'Force Push (with lease)',
      group: 'Git',
      icon: ShieldAlertIcon,
      keywords: ['git', 'force', 'push', 'lease'],
      dangerous: true,
      visible: activeFolderVisible,
      run: forcePushActiveFolder
    }),
    appCommand({
      id: 'git-undo-last-commit',
      label: 'Undo Last Commit',
      group: 'Git',
      icon: Undo2Icon,
      keywords: ['git', 'undo', 'reset', 'revert', 'commit'],
      dangerous: true,
      visible: () => {
        const folderPath = getActiveFolderPath()
        return folderPath !== null && !!getGitSummary(folderPath)?.branch
      },
      run: undoLastCommitActiveFolder
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
      visible: activeFolderVisible,
      run: () => {
        const folderPath = getActiveFolderPath()
        if (folderPath) addNotificationToolTab(folderPath)
      }
    })
  ].filter((definition) => !definition.capability || platform.capabilities[definition.capability])
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
