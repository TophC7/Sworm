import type { Command, CommandGroup } from './types'
import { BellIcon, XIcon } from '$lib/icons/lucideExports'
import {
  clearAllNotifications,
  getNotifications,
  isNotificationCenterOpen,
  setNotificationCenterOpen
} from '$lib/stores/notifications.svelte'
import { addNotificationTestTab, getActiveProjectId } from '$lib/stores/workspace.svelte'

export function getNotificationCommands(): CommandGroup[] {
  const notifications = getNotifications()
  const expanded = isNotificationCenterOpen()
  const activeProjectId = getActiveProjectId()
  const toggleNotificationLabel = expanded ? 'Hide Notifications' : 'Open Notifications'
  const commands: Command[] = [
    {
      id: 'open-notifications',
      label: toggleNotificationLabel,
      icon: BellIcon,
      keywords: ['notification', 'notifications', 'toast', 'alerts', 'open', 'show', 'hide', 'toggle'],
      onSelect: () => setNotificationCenterOpen(!expanded)
    }
  ]

  if (activeProjectId) {
    commands.push({
      id: 'open-notification-tester',
      label: 'Open Notification Tester',
      icon: BellIcon,
      keywords: ['notification', 'notifications', 'tester', 'preview', 'debug', 'demo', 'test'],
      onSelect: () => addNotificationTestTab(activeProjectId)
    })
  }

  if (notifications.length > 0) {
    commands.push({
      id: 'dismiss-notifications',
      label: 'Dismiss Notifications',
      icon: XIcon,
      keywords: ['notification', 'notifications', 'toast', 'alerts', 'dismiss', 'clear', 'remove'],
      onSelect: clearAllNotifications
    })
  }

  return [
    {
      heading: 'Notifications',
      commands
    }
  ]
}
