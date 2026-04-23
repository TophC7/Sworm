// Project-config bootstrap.
//
// Pulls every JSON schema declared by the Rust backend and pushes it
// into the Monaco schema registry. Called once on app boot. After this
// runs, opening any matching config file (e.g. `.sworm/tasks.json`)
// gets autocomplete, validation, and hover docs for free.
//
// Adding a new config schema is a backend-only change:
// <> define the type with `#[derive(JsonSchema)]`
// <> append an entry to `all_config_schemas()`
// <> no frontend change required

import { backend } from '$lib/api/backend'
import { registerSchema } from '$lib/features/editor/schemas/registry'

let bootstrapped: Promise<void> | null = null

export function initProjectSchemas(): Promise<void> {
  if (bootstrapped) return bootstrapped
  bootstrapped = (async () => {
    const entries = await backend.configSchemas.list()
    for (const entry of entries) {
      registerSchema({
        id: entry.id,
        fileMatch: entry.fileMatch,
        schema: entry.schema
      })
    }
  })().catch((err) => {
    // Reset so a subsequent attempt can retry on next app mount.
    bootstrapped = null
    throw err
  })
  return bootstrapped
}
