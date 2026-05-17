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

    const formatter = await resolveFormatterSelection(group, context.projectPath)
    if (formatter === 'disabled') return []
    if (formatter === 'lsp') {
      return formatDocumentWithLsp(model)
    }

    try {
      if (formatter === 'biome') {
        const filePath = fileUriToPath(model)
        if (!filePath) return []
        const formatted = await backend.formatting.biome(context.projectId, filePath, model.getValue())
        return toFullDocumentEdit(model, formatted)
      }

      if (formatter === 'nixfmt') {
        const formatted = await backend.formatting.nixfmt(context.projectId, model.getValue())
        return toFullDocumentEdit(model, formatted)
      }
    } catch (error) {
      console.warn(`Formatter ${formatter} failed`, error)
    }

    return []
  }
}

const registry = new FormatterRegistry()
const formatterSettingsByProjectPath = new Map<string, Promise<FormattingSettings>>()
let formatterSettingsInvalidationStarted = false
let formatterSettingsCachingEnabled = true

export function ensureMonacoFormatters(monaco: Monaco) {
  return registry.ensureMonaco(monaco)
}

async function resolveFormatterSelection(group: FormattingGroupId, projectPath: string): Promise<FormatterSelection> {
  try {
    const formatting = await resolveProjectFormattingSettings(projectPath)
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
      formatterSettingsByProjectPath.clear()
    })
    .catch(() => {
      formatterSettingsCachingEnabled = false
      formatterSettingsByProjectPath.clear()
    })
}

async function resolveProjectFormattingSettings(projectPath: string): Promise<FormattingSettings> {
  if (!formatterSettingsCachingEnabled) {
    const effective = await backend.settings.getEffective(projectPath)
    return effective.settings.formatting
  }

  let cached = formatterSettingsByProjectPath.get(projectPath)
  if (!cached) {
    cached = backend.settings
      .getEffective(projectPath)
      .then((effective) => effective.settings.formatting)
      .catch((error) => {
        formatterSettingsByProjectPath.delete(projectPath)
        throw error
      })
    formatterSettingsByProjectPath.set(projectPath, cached)
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
