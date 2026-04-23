// Central JSON Schema registry for Monaco.
//
// Any module that wants to give Monaco autocomplete/validation for a
// JSON file registers an entry here. The registry buffers entries until
// Monaco is loaded, then flushes them to `jsonDefaults`. Subsequent
// mutations sync immediately.
//
// Why this exists:
// <> Monaco's jsonDefaults is a global singleton. Multiple callers
//    setting `schemas` would clobber each other without a coordinator.
// <> Schemas may be registered before Monaco has been imported (app
//    boot fetches them from the backend). The registry buffers until
//    `attachMonaco` is called from the first editor mount.

export interface SchemaEntry {
  /** Stable identifier, e.g. `sworm.tasks` or `settings:lsp.typescript`. */
  id: string
  /** Globs matched against model URIs, e.g. `['**\/.sworm/tasks.json']`. */
  fileMatch: string[]
  /** JSON Schema object. */
  schema: unknown
}

interface JsonSchemaAssociation {
  uri: string
  fileMatch?: string[]
  schema: unknown
}

interface JsonDiagnosticsOptions {
  validate?: boolean
  allowComments?: boolean
  schemas?: JsonSchemaAssociation[]
}

interface JsonDefaults {
  diagnosticsOptions?: JsonDiagnosticsOptions
  setDiagnosticsOptions(options: JsonDiagnosticsOptions): void
}

const entries = new Map<string, SchemaEntry>()
let monacoRef: typeof import('monaco-editor') | null = null

function jsonDefaults(monaco: typeof import('monaco-editor')): JsonDefaults {
  return (monaco.languages.json as unknown as { jsonDefaults: JsonDefaults }).jsonDefaults
}

function syncNow(): void {
  if (!monacoRef) return
  const defaults = jsonDefaults(monacoRef)
  const current = defaults.diagnosticsOptions ?? {}
  defaults.setDiagnosticsOptions({
    ...current,
    validate: true,
    allowComments: true,
    schemas: Array.from(entries.values()).map((entry) => ({
      uri: `sworm-schemas://${encodeURIComponent(entry.id)}`,
      fileMatch: entry.fileMatch,
      schema: entry.schema
    }))
  })
}

/**
 * Called by `initMonaco` on first editor mount. Idempotent. Flushes
 * any entries buffered before Monaco loaded.
 */
export function attachMonaco(monaco: typeof import('monaco-editor')): void {
  if (monacoRef === monaco) return
  monacoRef = monaco
  syncNow()
}

export function registerSchema(entry: SchemaEntry): void {
  entries.set(entry.id, entry)
  syncNow()
}

export function unregisterSchema(id: string): void {
  if (entries.delete(id)) syncNow()
}
