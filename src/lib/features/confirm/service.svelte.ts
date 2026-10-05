// Promise-based confirm dialog.
//
// Tauri v2's webview doesn't reliably render native `window.confirm()`
// on Linux — on some configurations it silently returns true without
// showing anything. ConfirmHost renders these prompts with the project's
// Svelte dialog wrappers instead, so they are always visible.
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
  run?: () => Promise<void>
}

interface PendingConfirm extends ConfirmRequest {
  id: number
  resolve: (value: boolean) => void
  reject: (reason: unknown) => void
  busy: boolean
}

let nextId = 0
const queue = $state<PendingConfirm[]>([])

export function getPendingConfirm(): PendingConfirm | null {
  return queue[0] ?? null
}

export function confirmAsync(request: ConfirmRequest): Promise<boolean> {
  return new Promise<boolean>((resolve, reject) => {
    queue.push({ ...request, id: nextId++, resolve, reject, busy: false })
  })
}

export async function resolvePendingConfirm(id: number, value: boolean): Promise<void> {
  const current = queue[0]
  // Late close/activation callbacks belong to their request, never the next head.
  if (!current || current.id !== id || current.busy) return
  if (!value || !current.run) {
    queue.shift()
    current.resolve(value)
    return
  }

  current.busy = true
  try {
    await current.run()
    queue.shift()
    current.resolve(true)
  } catch (error) {
    queue.shift()
    current.reject(error)
  }
}
