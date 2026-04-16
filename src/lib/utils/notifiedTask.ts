import { dismissNotification, notify, type NotificationTone } from '$lib/stores/notifications.svelte'

type MessageResolver<T> = string | ((value: T) => string | undefined)

interface NotificationStage<T> {
  title: MessageResolver<T>
  description?: MessageResolver<T>
  tone?: NotificationTone
}

export interface RunNotifiedTaskOptions<T> {
  loading: {
    title: string
    description?: string
  }
  success?: NotificationStage<T>
  error?: NotificationStage<unknown>
}

export function getErrorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error)
}

function resolveMessage<T>(message: MessageResolver<T> | undefined, value: T): string | undefined {
  if (typeof message === 'function') return message(value)
  return message
}

export async function runNotifiedTask<T>(
  task: () => Promise<T>,
  options: RunNotifiedTaskOptions<T>
): Promise<T | undefined> {
  const notificationId = notify.loading(options.loading.title, options.loading.description)

  try {
    const result = await task()

    if (!options.success) {
      dismissNotification(notificationId)
      return result
    }

    notify.update(notificationId, {
      title: resolveMessage(options.success.title, result) ?? options.loading.title,
      description: resolveMessage(options.success.description, result),
      tone: options.success.tone ?? 'success',
      loading: false
    })

    return result
  } catch (error) {
    notify.update(notificationId, {
      title: resolveMessage(options.error?.title, error) ?? options.loading.title,
      description: resolveMessage(options.error?.description, error) ?? getErrorMessage(error),
      tone: options.error?.tone ?? 'error',
      loading: false
    })

    return undefined
  }
}
