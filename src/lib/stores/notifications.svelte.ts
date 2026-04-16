// App-wide notification store surfaced by a single bottom-right notification surface.
//
// Notifications stay in `notifications` until explicitly dismissed.
// `active` controls whether a notification is currently visible in the collapsed view.

export type NotificationTone = 'neutral' | 'success' | 'warning' | 'error'

export interface NotificationAction {
  label: string
  onSelect: () => void
  disabled?: boolean
  destructive?: boolean
}

export interface NotificationMenuAction extends NotificationAction {
  kind: 'menu'
  items: NotificationAction[]
}

export type NotificationPrimaryAction = NotificationAction | NotificationMenuAction
export type NotificationSecondaryAction = NotificationAction

export interface NotificationOptions {
  description?: string
  loading?: boolean
  progress?: number
  primaryAction?: NotificationPrimaryAction
  secondaryAction?: NotificationSecondaryAction
}

export interface NotificationUpdate {
  tone?: NotificationTone
  title?: string
  description?: string
  loading?: boolean
  progress?: number | null
  primaryAction?: NotificationPrimaryAction | null
  secondaryAction?: NotificationSecondaryAction | null
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
  progress?: number
  primaryAction?: NotificationPrimaryAction
  secondaryAction?: NotificationSecondaryAction
}

type NotificationWriter = (title: string, description?: string, options?: NotificationOptions) => string

const TOAST_DURATION_MS = 4000

let notifications = $state<Notification[]>([])
let notificationCenterOpen = $state(false)
let nextId = 0
const hideTokens = new Map<string, number>()

function createNotification(tone: NotificationTone, title: string, options: NotificationOptions = {}): Notification {
  const timestamp = Date.now()

  return {
    id: `n-${Date.now()}-${nextId++}`,
    tone,
    title,
    description: options.description,
    timestamp,
    updatedAt: timestamp,
    active: true,
    loading: options.loading ?? false,
    progress: normalizeProgress(options.loading ?? false, options.progress),
    primaryAction: options.primaryAction,
    secondaryAction: options.secondaryAction
  }
}

function withDescription(
  description: string | undefined,
  options: NotificationOptions,
  overrides: NotificationOptions = {}
): NotificationOptions {
  return { ...options, ...overrides, description }
}

function removeById(list: Notification[], id: string): void {
  const index = list.findIndex((notification) => notification.id === id)
  if (index !== -1) list.splice(index, 1)
}

function sortNotifications(list: Notification[]): Notification[] {
  return list
    .slice()
    .sort((a, b) => (a.updatedAt === b.updatedAt ? a.timestamp - b.timestamp : a.updatedAt - b.updatedAt))
}

function getNotificationById(id: string): Notification | undefined {
  return notifications.find((notification) => notification.id === id)
}

function setNotificationActive(id: string, active: boolean): void {
  const notification = getNotificationById(id)
  if (notification) notification.active = active
}

function nextHideToken(id: string): number {
  const token = (hideTokens.get(id) ?? 0) + 1
  hideTokens.set(id, token)
  return token
}

function normalizeProgress(loading: boolean, progress?: number | null): number | undefined {
  if (!loading || progress == null) return undefined
  return Math.min(100, Math.max(0, progress))
}

function scheduleNotificationHide(id: string): void {
  const token = nextHideToken(id)

  setTimeout(() => {
    if (hideTokens.get(id) !== token) return
    setNotificationActive(id, false)
  }, TOAST_DURATION_MS)
}

function keepNotificationVisible(id: string): void {
  nextHideToken(id)
}

function refreshNotificationVisibility(notification: Notification): void {
  if (notification.loading) {
    keepNotificationVisible(notification.id)
    return
  }

  scheduleNotificationHide(notification.id)
}

function reactivateNotification(notification: Notification): void {
  notification.updatedAt = Date.now()
  notification.active = true
  refreshNotificationVisibility(notification)
}

function applyNotificationUpdate(notification: Notification, update: NotificationUpdate): void {
  if (update.tone !== undefined) notification.tone = update.tone
  if (update.title !== undefined) notification.title = update.title
  if (update.description !== undefined) notification.description = update.description

  if (update.loading !== undefined) {
    notification.loading = update.loading
    if (update.loading && update.tone === undefined) notification.tone = 'neutral'
  }

  if (update.progress !== undefined) {
    notification.progress = normalizeProgress(notification.loading, update.progress)
  } else if (!notification.loading) {
    notification.progress = undefined
  }

  if (update.primaryAction !== undefined) {
    notification.primaryAction = update.primaryAction ?? undefined
  }

  if (update.secondaryAction !== undefined) {
    notification.secondaryAction = update.secondaryAction ?? undefined
  }
}

function createNotificationEntry(tone: NotificationTone, title: string, options: NotificationOptions = {}): string {
  const notification = createNotification(tone, title, options)
  notifications.push(notification)
  reactivateNotification(notification)

  return notification.id
}

function updateNotification(id: string, update: NotificationUpdate): void {
  const notification = getNotificationById(id)
  if (!notification) return

  applyNotificationUpdate(notification, update)
  reactivateNotification(notification)
}

function createNotifier(tone: NotificationTone, defaults: NotificationOptions = {}): NotificationWriter {
  return (title, description, options = {}) =>
    createNotificationEntry(tone, title, withDescription(description, options, defaults))
}

function getSortedNotifications(predicate?: (notification: Notification) => boolean): Notification[] {
  const notificationList = predicate ? notifications.filter(predicate) : notifications
  return sortNotifications(notificationList)
}

export const notify = {
  info: createNotifier('neutral'),
  success: createNotifier('success'),
  warning: createNotifier('warning'),
  error: createNotifier('error'),
  loading: createNotifier('neutral', { loading: true }),
  update: updateNotification
}

export function getNotifications(): Notification[] {
  return getSortedNotifications()
}

export function getActiveNotifications(): Notification[] {
  return getSortedNotifications((notification) => notification.active)
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
  hideTokens.delete(id)
  removeById(notifications, id)
}

export function clearAllNotifications(): void {
  hideTokens.clear()
  notifications.length = 0
}
