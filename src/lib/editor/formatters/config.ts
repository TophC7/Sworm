import { getBuiltinSettingsPageForGroup, getBuiltinSettingsPages } from '$lib/builtins/catalog'
import type { FormatterSelection } from '$lib/types/backend'

export type FormattingGroupId = 'javascript_typescript' | 'json' | 'nix'

export function formatterManagedLanguageIds(): string[] {
  return [
    ...new Set(
      getBuiltinSettingsPages()
        .filter((page) => page.formatter)
        .flatMap((page) => page.language_ids)
    )
  ]
}

export function isFormatterManagedLanguage(languageId: string): boolean {
  return formattingGroupForLanguageId(languageId) !== null
}

export function formattingGroupForLanguageId(languageId: string): FormattingGroupId | null {
  return (
    getBuiltinSettingsPages().find((page) => page.formatter && page.language_ids.includes(languageId))
      ?.formatter?.group ?? null
  )
}

export function defaultFormatterForGroup(group: FormattingGroupId): FormatterSelection {
  return getBuiltinSettingsPageForGroup(group)?.formatter?.default ?? fallbackDefault(group)
}

export function formatterOptionsForGroup(group: FormattingGroupId): FormatterSelection[] {
  return getBuiltinSettingsPageForGroup(group)?.formatter?.options ?? []
}

function fallbackDefault(group: FormattingGroupId): FormatterSelection {
  switch (group) {
    case 'javascript_typescript':
    case 'json':
      return 'biome'
    case 'nix':
      return 'nixfmt'
  }
}
