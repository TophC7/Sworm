import type { FormatterSelection } from '$lib/types/backend'

export type FormattingGroupId = 'javascript_typescript' | 'json' | 'nix'

export const FORMATTER_MANAGED_LANGUAGE_IDS = new Set(['javascript', 'typescript', 'json', 'nix'])

export function formattingGroupForLanguageId(languageId: string): FormattingGroupId | null {
  switch (languageId) {
    case 'javascript':
    case 'typescript':
      return 'javascript_typescript'
    case 'json':
      return 'json'
    case 'nix':
      return 'nix'
    default:
      return null
  }
}

export function defaultFormatterForGroup(group: FormattingGroupId): Exclude<FormatterSelection, 'auto'> {
  switch (group) {
    case 'javascript_typescript':
    case 'json':
      return 'biome'
    case 'nix':
      return 'nixfmt'
  }
}
