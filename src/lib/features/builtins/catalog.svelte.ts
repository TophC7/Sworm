import { backend } from '$lib/api/backend'
import { basename } from '$lib/utils/paths'
import type {
  BuiltinCatalog,
  BuiltinLanguageContribution,
  BuiltinSettingsPage
} from '$lib/types/backend'

let catalog = $state.raw<BuiltinCatalog | null>(null)
let loadPromise: Promise<BuiltinCatalog> | null = null

export async function preloadBuiltinCatalog(): Promise<BuiltinCatalog> {
  if (loadPromise) return loadPromise

  loadPromise = backend.builtins
    .getCatalog()
    .then((result) => {
      catalog = result
      return result
    })
    .catch((error) => {
      loadPromise = null
      throw error
    })

  return loadPromise
}

export function getBuiltinRuntimeLanguages(): BuiltinLanguageContribution[] {
  return catalog?.runtime.languages ?? []
}

export function getBuiltinSettingsPages(): BuiltinSettingsPage[] {
  return catalog?.settings.pages ?? []
}

export function getBuiltinLanguageForFilePath(filePath: string): string | null {
  const fileName = basename(filePath)
  const extension = getNormalizedExtension(fileName)

  for (const contribution of getBuiltinRuntimeLanguages()) {
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
