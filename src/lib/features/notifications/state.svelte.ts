// App-wide notification store surfaced by a single bottom-right notification surface.
//
// Notifications stay in `notifications` until explicitly dismissed.
// `active` controls whether a notification is currently visible in the collapsed view.

export type NotificationTone = 'neutral' | 'success' | 'warning' | 'error'

export interface NotificationOptions {
  description?: string
  loading?: boolean
}

export interface NotificationUpdate {
  tone?: NotificationTone
  title?: string
  description?: string
  loading?: boolean
}

export interface Notification {
  id: string
  tone: NotificationTone
  title: string
  description?: string
  timestamp: number
  updatedAt: number
  active: boolean
  loading: boolean
}

const TOAST_DURATION_MS = 4000

let notifications = $state<Notification[]>([])
let notificationCenterOpen = $state(false)
let nextId = 0
const hideTimers = new Map<string, ReturnType<typeof setTimeout>>()

function show(notification: Notification): void {
  notification.updatedAt = Date.now()
  notification.active = true
  clearTimeout(hideTimers.get(notification.id))
  hideTimers.delete(notification.id)
  if (!notification.loading) {
    hideTimers.set(notification.id, setTimeout(() => {
      hideTimers.delete(notification.id)
      notification.active = false
    }, TOAST_DURATION_MS))
  }
}

function sortNotifications(list: Notification[]): Notification[] {
  return list
    .slice()
    .sort((a, b) => (a.updatedAt === b.updatedAt ? a.timestamp - b.timestamp : a.updatedAt - b.updatedAt))
}

function createNotificationEntry(tone: NotificationTone, title: string, options: NotificationOptions = {}): string {
  const timestamp = Date.now()
  const id = `n-${timestamp}-${nextId++}`
  notifications.push({
    id,
    tone,
    title,
    description: options.description,
    timestamp,
    updatedAt: timestamp,
    active: true,
    loading: options.loading ?? false
  })
  show(notifications[notifications.length - 1])
  return id
}

function updateNotification(id: string, update: NotificationUpdate): void {
  const notification = notifications.find((notification) => notification.id === id)
  if (!notification) return

  if (update.tone !== undefined) notification.tone = update.tone
  if (update.title !== undefined) notification.title = update.title
  if (update.description !== undefined) notification.description = update.description

  if (update.loading !== undefined) {
    notification.loading = update.loading
    if (update.loading && update.tone === undefined) notification.tone = 'neutral'
  }

  show(notification)
}

export const notify = {
  info: (title: string, description?: string, options: NotificationOptions = {}) =>
    createNotificationEntry('neutral', title, { ...options, description }),
  success: (title: string, description?: string, options: NotificationOptions = {}) =>
    createNotificationEntry('success', title, { ...options, description }),
  warning: (title: string, description?: string, options: NotificationOptions = {}) =>
    createNotificationEntry('warning', title, { ...options, description }),
  error: (title: string, description?: string, options: NotificationOptions = {}) =>
    createNotificationEntry('error', title, { ...options, description }),
  loading: (title: string, description?: string, options: NotificationOptions = {}) =>
    createNotificationEntry('neutral', title, { ...options, description, loading: true }),
  update: updateNotification
}

export function getNotifications(): Notification[] {
  return sortNotifications(notifications)
}

export function getActiveNotifications(): Notification[] {
  return sortNotifications(notifications.filter((notification) => notification.active))
}

export function isNotificationCenterOpen(): boolean {
  return notificationCenterOpen
}

export function setNotificationCenterOpen(open: boolean): void {
  notificationCenterOpen = open
}

export function toggleNotificationCenter(): void {
  notificationCenterOpen = !notificationCenterOpen
}

export function dismissNotification(id: string): void {
  clearTimeout(hideTimers.get(id))
  hideTimers.delete(id)
  const index = notifications.findIndex((notification) => notification.id === id)
  if (index !== -1) notifications.splice(index, 1)
}

export function clearAllNotifications(): void {
  for (const timer of hideTimers.values()) clearTimeout(timer)
  hideTimers.clear()
  notifications.length = 0
}
