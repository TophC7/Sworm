/**
 * Session status display helpers.
 */

import { allProviders, directOptions } from '$lib/data/providers'

/** Derive label from the canonical provider lists. */
export function providerLabel(providerId: string): string {
  return (
    allProviders.find((p) => p.id === providerId)?.label ??
    directOptions.find((p) => p.id === providerId)?.label ??
    providerId
  )
}
