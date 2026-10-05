import { backend } from '$lib/api/backend'
import { formatterManagedLanguageIds, formatterPolicyForLanguage } from '$lib/features/editor/formatters/config'
import { formatDocumentWithLsp, getLspDocumentContext } from '$lib/features/editor/lsp/registry'
import { modelHostPath } from '$lib/features/editor/renderers/monaco/text/modelCache'
import { preloadBuiltinCatalog } from '$lib/features/builtins/catalog.svelte'
import { getSettings, loadSettings } from '$lib/features/settings/state/settings.svelte'
import type { BuiltinFormatterPolicy, FormatterSelection, FormattingSettings } from '$lib/types/backend'

type Monaco = typeof import('monaco-editor')
type MonacoModel = import('monaco-editor').editor.ITextModel
type MonacoTextEdit = import('monaco-editor').languages.TextEdit

const registeredLanguages = new Set<string>()

export async function ensureMonacoFormatters(monaco: Monaco): Promise<void> {
  await preloadBuiltinCatalog()
  ensureFormatterSettingsCacheInvalidation()
  if (!getSettings()) {
    void loadSettings()
  }

  for (const languageId of formatterManagedLanguageIds()) {
    if (registeredLanguages.has(languageId)) continue
    registeredLanguages.add(languageId)
    monaco.languages.registerDocumentFormattingEditProvider(languageId, {
      provideDocumentFormattingEdits
    })
  }
}

async function provideDocumentFormattingEdits(model: MonacoModel): Promise<MonacoTextEdit[]> {
  const policy = formatterPolicyForLanguage(model.getLanguageId())
  if (!policy) return []

  const context = getLspDocumentContext(model)
  if (!context) return []

  const formatter = await resolveFormatterSelection(policy, context.folderPath)
  if (formatter === 'disabled') return []
  if (formatter === 'lsp') {
    return formatDocumentWithLsp(model)
  }

  try {
    if (formatter === 'biome') {
      const filePath = modelHostPath(model.uri)
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

const formatterSettingsByFolder = new Map<string, Promise<FormattingSettings>>()
let formatterSettingsInvalidationStarted = false
let formatterSettingsCachingEnabled = true

async function resolveFormatterSelection(policy: BuiltinFormatterPolicy, folderPath: string): Promise<FormatterSelection> {
  try {
    const formatting = await resolveProjectFormattingSettings(folderPath)
    return formatting[policy.group]?.formatter ?? policy.default
  } catch (error) {
    console.warn('Failed to load project-effective formatter settings', error)
    const settings = getSettings()?.formatting
    return settings?.[policy.group]?.formatter ?? policy.default
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
