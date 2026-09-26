import { backend } from '$lib/api/backend'
import { MAX_FILE_BYTES, type FileBytes, type FileReadRequest } from '$lib/api/transport.web.files'
import { mediaMimeType } from '$lib/features/editor/languageMap'
import type { Platform } from './index'

export function createWebPlatform(
  workbenchId: string,
  options: {
    closeCurrent: () => Promise<void>
    readFileBytes: (request: FileReadRequest) => Promise<FileBytes>
  }
): Platform {
  return {
    workbench: { id: workbenchId, closeCurrent: options.closeCurrent },
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
      async load(folderPath: string, filePath: string, signal: AbortSignal) {
        signal.throwIfAborted()
        const stat = await backend.files.stat(folderPath, filePath)
        signal.throwIfAborted()
        if (!stat.regular) throw new Error('Not a regular file')
        if (stat.size > MAX_FILE_BYTES)
          throw new Error(`File exceeds the ${MAX_FILE_BYTES / (1024 * 1024)} MiB media limit`)

        const requestId = crypto.randomUUID()
        const cancel = () => void backend.files.cancelReadStream(requestId).catch(() => {})
        signal.addEventListener('abort', cancel, { once: true })
        const { bytes } = await options
          .readFileBytes({ requestId, projectPath: folderPath, filePath, version: stat.version, size: stat.size })
          .finally(() => signal.removeEventListener('abort', cancel))
        signal.throwIfAborted()
        const url = URL.createObjectURL(new Blob([bytes], { type: mediaMimeType(filePath) }))
        return { url, dispose: () => URL.revokeObjectURL(url) }
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
      nativeFileClipboard: false,
      nativeDirectoryPicker: false,
      externalFileOpen: false,
      fileClaims: false,
      remoteHosts: false,
      durableWorkbenches: true
    },
    native: null
  }
}
