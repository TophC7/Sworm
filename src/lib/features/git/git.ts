/** Parse a stash message ("On <branch>: <msg>" or "WIP on <branch>: <msg>")
 *  into its branch and description parts. */
export function parseStashMessage(raw: string): { branch: string | null; label: string } {
  const match = raw.match(/^(?:WIP )?[Oo]n ([^:]+):\s*(.*)$/)
  if (match) {
    return { branch: match[1], label: match[2] || 'WIP' }
  }
  return { branch: null, label: raw }
}
