import type { HandleClientError } from '@sveltejs/kit'
import { describeClientError, logClientError } from '$lib/utils/client-error'

export const handleError: HandleClientError = ({ error, event, status, message }) => {
  logClientError('client navigation error', {
    status,
    message,
    url: event.url.href,
    routeId: event.route.id,
    error,
    detail: describeClientError(error)
  })

  return { message }
}
