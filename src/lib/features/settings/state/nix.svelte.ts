// Per-project Nix environment state using Svelte 5 runes.

import { backend } from '$lib/api/backend'
import { refreshLspProjectEnvironment } from '$lib/features/editor/lsp/registry'
import type { NixDetection, NixEnvRecord } from '$lib/types/backend'

let nixState = $state<Map<string, NixDetection>>(new Map())
let evaluating = $state<Set<string>>(new Set())

export function getNixDetection(projectId: string): NixDetection | undefined {
  return nixState.get(projectId)
}

export function isNixEvaluating(projectId: string): boolean {
  return evaluating.has(projectId)
}

export async function detectNix(projectId: string): Promise<NixDetection> {
  const detection = await backend.nix.detect(projectId)
  nixState.set(projectId, detection)
  nixState = new Map(nixState)
  return detection
}

export async function selectNixFile(projectId: string, nixFile: string): Promise<NixEnvRecord> {
  const record = await backend.nix.select(projectId, nixFile)
  const current = nixState.get(projectId)
  if (current) {
    nixState.set(projectId, { ...current, selected: record })
    nixState = new Map(nixState)
  }
  await refreshLspProjectEnvironment(projectId)
  return record
}

export async function evaluateNix(projectId: string): Promise<NixEnvRecord> {
  evaluating.add(projectId)
  evaluating = new Set(evaluating)
  try {
    const record = await backend.nix.evaluate(projectId)
    const current = nixState.get(projectId)
    if (current) {
      nixState.set(projectId, { ...current, selected: record })
      nixState = new Map(nixState)
    }
    await refreshLspProjectEnvironment(projectId)
    return record
  } finally {
    evaluating.delete(projectId)
    evaluating = new Set(evaluating)
  }
}

export async function clearNix(projectId: string): Promise<void> {
  await backend.nix.clear(projectId)
  const current = nixState.get(projectId)
  if (current) {
    nixState.set(projectId, { ...current, selected: null })
    nixState = new Map(nixState)
  }
  await refreshLspProjectEnvironment(projectId)
}
