import { backend } from '$lib/api/backend'
import type { Platform } from './index'

export function createWebPlatform(workbenchId: string): Platform {
  return {
    workbench: { id: workbenchId },
    app: { version: async () => (await backend.app.runtimeInfo()).version },
    clipboard: {
      readText: () => navigator.clipboard.readText(),
      writeText: (text) => navigator.clipboard.writeText(text)
    },
    links: {
      async openExternal(value) {
        const url = new URL(value)
        if (!['http:', 'https:', 'mailto:'].includes(url.protocol)) {
          throw new Error(`Unsupported external URL scheme: ${url.protocol}`)
        }
        window.open(url.href, '_blank', 'noopener,noreferrer')
      }
    },
    focus: {
      onChanged(handler) {
        const focus = () => handler(true)
        const blur = () => handler(false)
        window.addEventListener('focus', focus)
        window.addEventListener('blur', blur)
        return Promise.resolve(() => {
          window.removeEventListener('focus', focus)
          window.removeEventListener('blur', blur)
        })
      }
    },
    assets: {
      url() {
        throw new Error('Local media is unavailable in this browser')
      }
    },
    capabilities: {
      revealInFileManager: false,
      openInTerminal: false,
      tabTransfer: false,
      deepLinks: false,
      osDragDrop: false,
      nativeWindowControls: false,
      zoom: false,
      saveAsDialog: false,
      localAssetUrls: false,
      nativeFileClipboard: false,
      nativeDirectoryPicker: false,
      externalFileOpen: false,
      fileClaims: false,
      remoteHosts: false
    },
    native: null
  }
}
