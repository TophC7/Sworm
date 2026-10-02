// Web URL scheme for durable workbenches: `/?workbench=<id>` hosts one
// workbench. An unknown id is created on arrival; `/` mints a fresh one.

export const WORKBENCH_PARAM = 'workbench'

export function workbenchHref(id: string): string {
  return `/?${new URLSearchParams({ [WORKBENCH_PARAM]: id })}`
}

// Takeover authority rides one navigation only: the arriving page consumes it
// before its first handshake, so a later reload or duplicated tab never inherits it.
const takeoverKey = (id: string) => `sworm:takeover:${id}`

/** Call only after dirty confirmation; disarm the document's guard before navigating. */
export function takeOverWorkbench(id: string, leave: () => void, reload = false): void {
  sessionStorage.setItem(takeoverKey(id), '')
  leave()
  if (reload) window.location.reload()
  else window.location.assign(workbenchHref(id))
}

/** True once per explicit Take Over; clears the request. */
export function consumeTakeover(id: string): boolean {
  const requested = sessionStorage.getItem(takeoverKey(id)) !== null
  sessionStorage.removeItem(takeoverKey(id))
  return requested
}
