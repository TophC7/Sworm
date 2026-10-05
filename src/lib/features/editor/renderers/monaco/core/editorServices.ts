import { platform } from '$lib/platform'
import { StandaloneServices } from 'monaco-editor/esm/vs/editor/standalone/browser/standaloneServices.js'

type Uri = import('monaco-editor').Uri

const typedText = new Map<string, string>()
let findText = ''
let resources: Uri[] = []

const clipboardService = {
  triggerPaste(): undefined {
    return undefined
  },

  async writeText(text: string, type?: string): Promise<void> {
    resources = []
    if (type) {
      typedText.set(type, text)
      return
    }
    await platform.clipboard.writeText(text)
  },

  async readText(type?: string): Promise<string> {
    if (type) return typedText.get(type) ?? ''
    return (await platform.clipboard.readText()) ?? ''
  },

  async readFindText(): Promise<string> {
    return findText
  },

  async writeFindText(text: string): Promise<void> {
    findText = text
  },

  async readResources(): Promise<Uri[]> {
    return resources
  },

  async writeResources(next: readonly Uri[]): Promise<void> {
    resources = [...next]
  },

  clearInternalState(): void {
    resources = []
  }
}

export function initializeMonacoEditorServices(): void {
  // Monaco's default service primes clipboard writes on every click/keydown.
  // Platform clipboard access does not need Monaco's gesture priming.
  StandaloneServices.initialize({ clipboardService })
}
