// Shared branch, stash and ref-formatting helpers.

export function splitRemoteBranchRef(name: string): { remote: string; branch: string } | null {
  const index = name.indexOf('/')
  if (index <= 0 || index === name.length - 1) return null
  return {
    remote: name.slice(0, index),
    branch: name.slice(index + 1)
  }
}

export function localNameForRemoteRef(name: string): string {
  return splitRemoteBranchRef(name)?.branch ?? name
}

/** Parse a stash message ("On <branch>: <msg>" or "WIP on <branch>: <msg>")
 *  into its branch and description parts. */
export function parseStashMessage(raw: string): { branch: string | null; label: string } {
  const match = raw.match(/^(?:WIP )?[Oo]n ([^:]+):\s*(.*)$/)
  if (match) {
    return { branch: match[1], label: match[2] || 'WIP' }
  }
  return { branch: null, label: raw }
}
