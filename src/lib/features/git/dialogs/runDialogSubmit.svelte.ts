import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

interface DialogSubmitOptions {
  run: () => Promise<void>
  onDone?: () => void
  onError?: (error: unknown) => boolean | void | Promise<boolean | void>
}

export function runDialogSubmit({ run, onDone, onError }: DialogSubmitOptions) {
  let busy = $state(false)
  let error = $state<string | null>(null)

  async function submit() {
    if (busy) return
    busy = true
    error = null
    try {
      await run()
      onDone?.()
    } catch (e) {
      if (await onError?.(e)) return
      error = getErrorMessage(e)
    } finally {
      busy = false
    }
  }

  function clearError() {
    error = null
  }

  return {
    get busy() {
      return busy
    },
    get error() {
      return error
    },
    submit,
    clearError
  }
}
