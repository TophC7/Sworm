// Promise-based confirm dialog.
//
// Tauri v2's webview doesn't reliably render native `window.confirm()`
// on Linux — on some configurations it silently returns true without
// showing anything. This module routes all "are you sure?" prompts
// through the project's ConfirmDialog primitive instead, which is a
// Svelte component and therefore always visible.
//
// Requests are FIFO-queued. Older callers used to auto-cancel prior
// pending requests, but that created a race where the second request's
// `resolve` could fire against the first dialog's close animation. A
// straight queue is simpler and matches user intuition: if two confirms
// land at once, the user answers them in order.
//
// Usage:
//   const ok = await confirmAsync({ title: '...', message: '...' })
//   if (!ok) return

export interface ConfirmRequest {
  title: string
  message: string
  confirmLabel?: string
  cancelLabel?: string
}

interface PendingConfirm extends ConfirmRequest {
  id: number
  resolve: (value: boolean) => void
}

let nextId = 0
const queue = $state<PendingConfirm[]>([])

export function getPendingConfirm(): PendingConfirm | null {
  return queue[0] ?? null
}

export function confirmAsync(request: ConfirmRequest): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    queue.push({ ...request, id: nextId++, resolve })
  })
}

export function resolvePendingConfirm(value: boolean): void {
  const current = queue.shift()
  if (!current) return
  current.resolve(value)
}
