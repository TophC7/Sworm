// Relative time and full date formatting for commit tooltips.

const MINUTE = 60_000
const HOUR = 60 * MINUTE
const DAY = 24 * HOUR
const WEEK = 7 * DAY
const MONTH = 30 * DAY
const YEAR = 365 * DAY

/** "now", "5m ago", "3h ago", "2d ago", etc. */
export function timeAgo(dateStr: string): string {
  const ms = Date.now() - new Date(dateStr).getTime()
  if (ms < MINUTE) return 'now'
  if (ms < HOUR) return `${Math.floor(ms / MINUTE)}m ago`
  if (ms < DAY) return `${Math.floor(ms / HOUR)}h ago`
  if (ms < WEEK) return `${Math.floor(ms / DAY)}d ago`
  if (ms < MONTH) return `${Math.floor(ms / WEEK)}w ago`
  if (ms < YEAR) return `${Math.floor(ms / MONTH)}mo ago`
  return `${Math.floor(ms / YEAR)}y ago`
}

/** "April 13, 2026 at 2:08 PM" */
export function formatFullDate(dateStr: string): string {
  return new Date(dateStr).toLocaleString('en-US', {
    year: 'numeric',
    month: 'long',
    day: 'numeric',
    hour: 'numeric',
    minute: '2-digit',
    hour12: true
  })
}
