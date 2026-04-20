import { backend } from '$lib/api/backend'
import type { LspExtensionEntry, LspServerCatalogEntry } from '$lib/types/backend'

let extensions: LspExtensionEntry[] = []
let loadPromise: Promise<LspExtensionEntry[]> | null = null

export async function preloadLspCatalog(): Promise<LspExtensionEntry[]> {
  if (loadPromise) return loadPromise

  loadPromise = backend.lsp
    .listExtensions()
    .then((result) => {
      extensions = result
      return result
    })
    .catch((error) => {
      loadPromise = null
      throw error
    })

  return loadPromise
}

export function invalidateLspCatalog() {
  loadPromise = null
  extensions = []
}

export function getLspExtensions(): LspExtensionEntry[] {
  return extensions
}

export function getLspCatalogServers(): LspServerCatalogEntry[] {
  return extensions.flatMap((extension) => extension.servers)
}

export function getExtensionLanguageForFilePath(filePath: string): string | null {
  const fileName = filePath.split('/').pop() ?? filePath
  const extension = getNormalizedExtension(fileName)

  for (const contribution of extensions.flatMap((entry) => entry.languages)) {
    if (contribution.filenames.some((name) => name === fileName)) {
      return contribution.id
    }
    if (extension && contribution.extensions.some((value) => normalizeExtension(value) === extension)) {
      return contribution.id
    }
  }

  return null
}

function getNormalizedExtension(fileName: string): string | null {
  const lastDot = fileName.lastIndexOf('.')
  if (lastDot < 0) return null
  return normalizeExtension(fileName.slice(lastDot))
}

function normalizeExtension(value: string): string {
  const trimmed = value.trim().toLowerCase()
  return trimmed.startsWith('.') ? trimmed : `.${trimmed}`
}
