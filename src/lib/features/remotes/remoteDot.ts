import type { RemoteStatus } from '$lib/types/backend'

export function remoteDotClass(state: RemoteStatus['state'] | 'checking'): string {
  switch (state) {
    case 'connected':
      return 'bg-success'
    case 'error':
      return 'bg-danger'
    case 'reconnecting':
      return 'bg-warning'
    default:
      return 'bg-muted'
  }
}
