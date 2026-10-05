import { getBuiltinSettingsPages } from '$lib/features/builtins/catalog.svelte'
import type { BuiltinFormatterPolicy } from '$lib/types/backend'

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
  return formatterPolicyForLanguage(languageId) !== null
}

export function formatterPolicyForLanguage(languageId: string): BuiltinFormatterPolicy | null {
  return (
    getBuiltinSettingsPages().find((page) => page.formatter && page.language_ids.includes(languageId))?.formatter ??
    null
  )
}
