import { setSettingsOpen, setSettingsPage } from '$lib/features/settings/dialog/state.svelte'

/**
 * The pairing form on the Remote Servers settings page. Module state so a
 * `sworm-pair:` deep link can fill it in before the page mounts. `name` is
 * null until the user picks one; the form suggests one from the link.
 */
export const pairForm = $state<{ input: string; name: string | null }>({ input: '', name: null })

export function openRemoteManager(): void {
  setSettingsPage('remote-servers')
  setSettingsOpen(true)
}

/** Show the Remote Servers page with a received pairing link filled in. */
export function openPairLink(link: string): void {
  pairForm.input = link
  pairForm.name = null
  openRemoteManager()
}

/** Preview only. The backend validates the original link before connecting. */
export function parsePairLink(link: string): { host: string; port: number; fingerprint: string; name: string } | null {
  try {
    const url = new URL(link)
    const parts = url.pathname.split('/')
    if (
      url.protocol !== 'sworm-pair:' ||
      !url.hostname ||
      url.username ||
      url.password ||
      url.search ||
      url.hash ||
      !url.port ||
      parts.length !== 3 ||
      !/^(?:SHA256:)?[a-fA-F0-9]{64}$/i.test(parts[1]) ||
      !/^[A-Za-z0-9_-]+$/.test(parts[2])
    )
      return null
    const port = Number(url.port)
    if (port < 1 || port > 65535) return null
    const host = url.hostname.replace(/^\[|\]$/g, '')
    if (!/^[A-Za-z0-9_.:-]+$/.test(host)) return null
    return { host, port, fingerprint: parts[1], name: host.replace(/[^A-Za-z0-9_-]/g, '-') || 'remote' }
  } catch {
    return null
  }
}

export function pairCommand(address: string): string | null {
  try {
    const url = new URL(`https://${address}`)
    if (url.username || url.password || url.pathname !== '/' || url.search || url.hash || !url.hostname) return null
    const host = url.hostname.replace(/^\[|\]$/g, '')
    return `ssh -- '${host.replace(/'/g, "'\\''")}' sworm-server pair`
  } catch {
    return null
  }
}
