export const NOTIFICATION_PANEL_WIDTH_CLASS = 'w-80'
export const NOTIFICATION_LIST_CLASS = 'flex flex-col gap-1.5 p-2'
export const NOTIFICATION_EMPTY_STATE_CLASS =
  'flex items-center justify-center px-3 py-6 text-center text-[0.72rem] text-subtle'
export const NOTIFICATION_SURFACE_CLASS =
  'pointer-events-auto overflow-hidden rounded-xl border border-edge bg-raised shadow-[0_10px_30px_rgba(0,0,0,0.45)]'
export const NOTIFICATION_VIEWPORT_CLASS = 'max-h-96 overflow-y-auto'
export const MAX_VISIBLE_ACTIVE_NOTIFICATIONS = 3

export function getNotificationProgressVariant(tone: 'neutral' | 'success' | 'warning' | 'error') {
  switch (tone) {
    case 'success':
      return 'success'
    case 'warning':
      return 'warning'
    case 'error':
      return 'danger'
    default:
      return 'accent'
  }
}

export function getNotificationAlertVariant(tone: 'neutral' | 'success' | 'warning' | 'error') {
  return tone === 'neutral' ? 'info' : tone
}

export function formatNotificationTimestamp(timestamp: number): string {
  const diff = Date.now() - timestamp

  if (diff < 60_000) return 'just now'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`

  return new Date(timestamp).toLocaleDateString()
}
