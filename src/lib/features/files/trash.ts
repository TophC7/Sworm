import { confirmAsync } from '$lib/features/confirm/service.svelte'

interface TrashUnavailableError {
  kind: 'trashUnavailable'
  message: string
}

function isTrashUnavailableError(error: unknown): error is TrashUnavailableError {
  return typeof error === 'object' && error !== null && 'kind' in error && error.kind === 'trashUnavailable'
}

/** Run a trash-first removal; if the OS trash refuses, ask, then rerun it permanently. False when declined. */
export async function withTrashFallback(remove: (permanent: boolean) => Promise<unknown>): Promise<boolean> {
  try {
    await remove(false)
    return true
  } catch (error) {
    if (!isTrashUnavailableError(error)) throw error
    const proceed = await confirmAsync({
      title: 'Delete Permanently?',
      message: `${error.message}. Delete permanently instead? This cannot be undone.`,
      confirmLabel: 'Delete Permanently'
    })
    if (proceed) await remove(true)
    return proceed
  }
}
