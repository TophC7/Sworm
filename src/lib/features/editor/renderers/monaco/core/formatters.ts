import { backend } from '$lib/api/backend'
import {
  defaultFormatterForGroup,
  formatterManagedLanguageIds,
  formattingGroupForLanguageId,
  type FormattingGroupId
} from '$lib/features/editor/formatters/config'
import { formatDocumentWithLsp, getLspDocumentContext } from '$lib/features/editor/lsp/registry'
import { preloadBuiltinCatalog } from '$lib/features/builtins/catalog'
import { getSettings, loadSettings } from '$lib/features/settings/state/settings.svelte'
import type { FormatterSelection, FormattingSettings } from '$lib/types/backend'

type Monaco = typeof import('monaco-editor')
type MonacoModel = import('monaco-editor').editor.ITextModel
type MonacoTextEdit = import('monaco-editor').languages.TextEdit

class FormatterRegistry {
  private monaco: Monaco | null = null
  private registeredLanguages = new Set<string>()

  async ensureMonaco(monaco: Monaco): Promise<void> {
    this.monaco = monaco
    await preloadBuiltinCatalog()
    ensureFormatterSettingsCacheInvalidation()
    if (!getSettings()) {
      void loadSettings()
    }

    for (const languageId of formatterManagedLanguageIds()) {
      if (this.registeredLanguages.has(languageId)) continue
      this.registeredLanguages.add(languageId)
      monaco.languages.registerDocumentFormattingEditProvider(languageId, {
        provideDocumentFormattingEdits: (model) => this.provideDocumentFormattingEdits(model)
      })
    }
  }

  private async provideDocumentFormattingEdits(model: MonacoModel): Promise<MonacoTextEdit[]> {
    const group = formattingGroupForLanguageId(model.getLanguageId())
    if (!group) return []

    const context = getLspDocumentContext(model)
    if (!context) return []

    const formatter = await resolveFormatterSelection(group, context.folderPath)
    if (formatter === 'disabled') return []
    if (formatter === 'lsp') {
      return formatDocumentWithLsp(model)
    }

    try {
      if (formatter === 'biome') {
        const filePath = fileUriToPath(model)
        if (!filePath) return []
        const formatted = await backend.formatting.biome(context.folderPath, filePath, model.getValue())
        return toFullDocumentEdit(model, formatted)
      }

      if (formatter === 'nixfmt') {
        const formatted = await backend.formatting.nixfmt(context.folderPath, model.getValue())
        return toFullDocumentEdit(model, formatted)
      }
    } catch (error) {
      console.warn(`Formatter ${formatter} failed`, error)
    }

    return []
  }
}

const registry = new FormatterRegistry()
const formatterSettingsByFolder = new Map<string, Promise<FormattingSettings>>()
let formatterSettingsInvalidationStarted = false
let formatterSettingsCachingEnabled = true

export function ensureMonacoFormatters(monaco: Monaco) {
  return registry.ensureMonaco(monaco)
}

async function resolveFormatterSelection(group: FormattingGroupId, folderPath: string): Promise<FormatterSelection> {
  try {
    const formatting = await resolveProjectFormattingSettings(folderPath)
    return formatting[group]?.formatter ?? defaultFormatterForGroup(group)
  } catch (error) {
    console.warn('Failed to load project-effective formatter settings', error)
    const settings = getSettings()?.formatting
    return settings?.[group]?.formatter ?? defaultFormatterForGroup(group)
  }
}

function ensureFormatterSettingsCacheInvalidation(): void {
  if (formatterSettingsInvalidationStarted) return
  formatterSettingsInvalidationStarted = true
  backend.settings
    .onChanged(() => {
      formatterSettingsByFolder.clear()
    })
    .catch(() => {
      formatterSettingsCachingEnabled = false
      formatterSettingsByFolder.clear()
    })
}

async function resolveProjectFormattingSettings(folderPath: string): Promise<FormattingSettings> {
  if (!formatterSettingsCachingEnabled) {
    const effective = await backend.settings.getEffective(folderPath)
    return effective.settings.formatting
  }

  let cached = formatterSettingsByFolder.get(folderPath)
  if (!cached) {
    cached = backend.settings
      .getEffective(folderPath)
      .then((effective) => effective.settings.formatting)
      .catch((error) => {
        formatterSettingsByFolder.delete(folderPath)
        throw error
      })
    formatterSettingsByFolder.set(folderPath, cached)
  }
  return cached
}

function toFullDocumentEdit(model: MonacoModel, formatted: string): MonacoTextEdit[] {
  if (formatted === model.getValue()) return []
  return [{ range: model.getFullModelRange(), text: formatted }]
}

function fileUriToPath(model: MonacoModel): string | null {
  return model.uri.scheme === 'file' ? model.uri.fsPath : null
}
