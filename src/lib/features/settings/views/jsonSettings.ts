import type { LspServerSettingsDescriptor } from '$lib/types/backend'

export interface JsonSettingsEditorSession {
  id: string
  title: string
  description: string
  schema: unknown
  defaults: unknown
  value: string
  onSave: (nextValue: string | null) => void | Promise<void>
}

function sharedPrefix(sharedScope: string | null | undefined): string {
  return sharedScope ? 'Shared ' : ''
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return value != null && typeof value === 'object' && !Array.isArray(value)
}

function parseJson(value: string | null | undefined): { ok: boolean; value: unknown } {
  if (!value?.trim()) return { ok: false, value: null }
  try {
    return { ok: true, value: JSON.parse(value) }
  } catch {
    return { ok: false, value: null }
  }
}

function schemaProperties(schema: unknown): Record<string, unknown> | null {
  if (!isPlainObject(schema)) return null
  return isPlainObject(schema.properties) ? schema.properties : null
}

function sectionName(descriptor: LspServerSettingsDescriptor | null | undefined): string | null {
  const section = descriptor?.section?.trim()
  return section ? section : null
}

export function usesSectionPayloadEditor(descriptor: LspServerSettingsDescriptor | null | undefined): boolean {
  const section = sectionName(descriptor)
  if (!section) return false

  const properties = schemaProperties(descriptor?.schema)
  if (properties) {
    if (section in properties) {
      return Object.keys(properties).length === 1
    }
    return true
  }

  const defaults = descriptor?.defaults
  if (!isPlainObject(defaults)) return false
  if (section in defaults) {
    return Object.keys(defaults).length === 1
  }
  return true
}

function unwrapSectionValue(descriptor: LspServerSettingsDescriptor | null | undefined, value: unknown): unknown {
  const section = sectionName(descriptor)
  if (!section || !usesSectionPayloadEditor(descriptor)) return value
  if (!isPlainObject(value)) return value
  return section in value ? value[section] : value
}

export function settingsEditorSchema(descriptor: LspServerSettingsDescriptor | null | undefined): unknown {
  if (!usesSectionPayloadEditor(descriptor)) {
    return descriptor?.schema ?? null
  }

  const section = sectionName(descriptor)
  const properties = schemaProperties(descriptor?.schema)
  if (!section || !properties || !(section in properties)) return descriptor?.schema ?? null
  return properties[section] ?? descriptor?.schema ?? null
}

export function settingsEditorDefaults(descriptor: LspServerSettingsDescriptor | null | undefined): unknown {
  return unwrapSectionValue(descriptor, descriptor?.defaults ?? null)
}

export function settingsEditorValue(
  descriptor: LspServerSettingsDescriptor | null | undefined,
  storedValue: string | null | undefined
): string {
  if (storedValue?.trim()) {
    const parsed = parseJson(storedValue)
    if (parsed.ok) return JSON.stringify(unwrapSectionValue(descriptor, parsed.value), null, 2)
    return storedValue
  }

  const defaults = settingsEditorDefaults(descriptor)
  return defaults != null ? JSON.stringify(defaults, null, 2) : ''
}

export function serializeSettingsEditorValue(
  descriptor: LspServerSettingsDescriptor | null | undefined,
  editorValue: string | null
): string | null {
  if (!editorValue?.trim()) return null

  const parsed = parseJson(editorValue)
  if (!parsed.ok) return null

  if (!usesSectionPayloadEditor(descriptor)) {
    return JSON.stringify(parsed.value, null, 2)
  }

  const section = sectionName(descriptor)
  if (!section) return JSON.stringify(parsed.value, null, 2)

  return JSON.stringify({ [section]: parsed.value }, null, 2)
}

export function settingsEditorDescription(
  descriptor: LspServerSettingsDescriptor | null | undefined,
  sharedScope?: string | null
): string {
  const section = sectionName(descriptor)
  if (!section) {
    return `${sharedPrefix(sharedScope)}server config.`
  }

  if (usesSectionPayloadEditor(descriptor)) {
    return `${sharedPrefix(sharedScope)}\`${section}\` config.`
  }

  return `${sharedPrefix(sharedScope)}server config.`
}
