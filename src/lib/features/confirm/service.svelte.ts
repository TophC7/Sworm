// Promise-based confirm, text prompt, and picker dialogs.
//
// Tauri v2's webview doesn't reliably render native `window.confirm()`
// on Linux — on some configurations it silently returns true without
// showing anything. ConfirmHost renders these prompts with the project's
// Svelte dialog wrappers instead, so they are always visible.
//
// Requests are FIFO-queued. Older callers used to auto-cancel prior
// pending requests, but that created a race where the second request's
// `resolve` could fire against the first dialog's close animation. A
// straight queue is simpler and matches user intuition: if two requests
// land at once, the user answers them in order.
//
// Usage:
//   const ok = await confirmAsync({ title: '...', message: '...' })
//   if (!ok) return
//   const answer = await promptAsync({ title: '...', value: '...' })
//   const choice = await pickAsync({ title: '...', options: [{ value: '...', label: '...' }] })

export interface ConfirmRequest {
  title: string
  message: string
  confirmLabel?: string
  cancelLabel?: string
  run?: () => Promise<void>
}

export interface PromptRequest {
  title: string
  message?: string
  value?: string
  placeholder?: string
  confirmLabel?: string
}

export interface PickOption {
  value: string
  label: string
  description?: string
}

/** Options render in order; the first takes initial focus. */
export interface PickRequest {
  title: string
  message?: string
  options: PickOption[]
}

type PendingRequest = { id: number; busy: boolean } & (
  | (ConfirmRequest & { kind: 'confirm'; resolve: (value: boolean) => void; reject: (reason: unknown) => void })
  | (PromptRequest & { kind: 'prompt'; resolve: (value: string | null) => void })
  | (PickRequest & { kind: 'pick'; resolve: (value: string | null) => void })
)

let nextId = 0
const queue = $state<PendingRequest[]>([])

export function getPendingRequest(): PendingRequest | null {
  return queue[0] ?? null
}

export function confirmAsync(request: ConfirmRequest): Promise<boolean> {
  return new Promise<boolean>((resolve, reject) => {
    queue.push({ ...request, kind: 'confirm', id: nextId++, resolve, reject, busy: false })
  })
}

export function promptAsync(request: PromptRequest): Promise<string | null> {
  return new Promise<string | null>((resolve) => {
    queue.push({ ...request, kind: 'prompt', id: nextId++, resolve, busy: false })
  })
}

export function pickAsync(request: PickRequest): Promise<string | null> {
  return new Promise<string | null>((resolve) => {
    queue.push({ ...request, kind: 'pick', id: nextId++, resolve, busy: false })
  })
}

export function resolvePendingAnswer(id: number, value: string | null): void {
  const current = queue[0]
  if (!current || current.id !== id || current.kind === 'confirm') return
  queue.shift()
  current.resolve(value)
}

export async function resolvePendingConfirm(id: number, value: boolean): Promise<void> {
  const current = queue[0]
  // Late close/activation callbacks belong to their request, never the next head.
  if (!current || current.id !== id || current.kind !== 'confirm' || current.busy) return
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
