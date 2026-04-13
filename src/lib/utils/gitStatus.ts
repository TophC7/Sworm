/** Normalize git status letter — maps ? to U for display. */
export function gitStatusDisplay(status: string): string {
  return status === '?' ? 'U' : status
}

/** Map a git status letter to a Tailwind text color class. */
export function gitStatusColor(status: string): string {
  switch (status) {
    case 'A':
      return 'text-success'
    case 'D':
      return 'text-danger'
    case 'R':
      return 'text-accent'
    case 'M':
      return 'text-warning'
    case '?':
      return 'text-success'
    default:
      return 'text-muted'
  }
}

/** Map a git status letter to a human-readable label. */
export function gitStatusLabel(status: string): string {
  switch (status) {
    case 'A':
      return 'Added'
    case 'D':
      return 'Deleted'
    case 'R':
      return 'Renamed'
    case 'M':
      return 'Modified'
    case '?':
      return 'Untracked'
    default:
      return 'Changed'
  }
}
