import { backend } from '$lib/api/backend'
import {
  defaultFormatterForGroup,
  formatterManagedLanguageIds,
  formattingGroupForLanguageId,
  type FormattingGroupId
} from '$lib/editor/formatters/config'
import { formatDocumentWithLsp, getLspDocumentContext } from '$lib/lsp/registry'
import { preloadBuiltinCatalog } from '$lib/builtins/catalog'
import { getSettings, loadSettings } from '$lib/stores/settings.svelte'
import type { FormatterSelection } from '$lib/types/backend'

type Monaco = typeof import('monaco-editor')
type MonacoModel = import('monaco-editor').editor.ITextModel
type MonacoTextEdit = import('monaco-editor').languages.TextEdit

class FormatterRegistry {
  private monaco: Monaco | null = null
  private registeredLanguages = new Set<string>()

  async ensureMonaco(monaco: Monaco): Promise<void> {
    this.monaco = monaco
    await preloadBuiltinCatalog()
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

    const formatter = resolveFormatterSelection(group)
    if (formatter === 'disabled') return []
    if (formatter === 'lsp') {
      return formatDocumentWithLsp(model)
    }

    const context = getLspDocumentContext(model)
    if (!context) return []

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

export function ensureMonacoFormatters(monaco: Monaco) {
  return registry.ensureMonaco(monaco)
}

function resolveFormatterSelection(group: FormattingGroupId): FormatterSelection {
  const settings = getSettings()?.formatting
  return settings?.[group]?.formatter ?? defaultFormatterForGroup(group)
}

function toFullDocumentEdit(model: MonacoModel, formatted: string): MonacoTextEdit[] {
  if (formatted === model.getValue()) return []
  return [{ range: model.getFullModelRange(), text: formatted }]
}

function fileUriToPath(model: MonacoModel): string | null {
  return model.uri.scheme === 'file' ? model.uri.fsPath : null
}
