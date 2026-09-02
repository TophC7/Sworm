// Per-folder Nix environment state using Svelte 5 runes.

import { backend } from '$lib/api/backend'
import { refreshLspFolderEnvironment } from '$lib/features/editor/lsp/registry'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import type { NixDetection, NixEnvRecord } from '$lib/types/backend'

const detections = createFolderKeyedStore<NixDetection>()
let evaluating = $state<Set<string>>(new Set())

export function getNixDetection(folderPath: string): NixDetection | undefined {
  return detections.get(folderPath)
}

export function isNixEvaluating(folderPath: string): boolean {
  return evaluating.has(folderPath)
}

export async function detectNix(folderPath: string): Promise<NixDetection> {
  const detection = await backend.nix.detect(folderPath)
  detections.set(folderPath, detection)
  return detection
}

export async function selectNixFile(folderPath: string, nixFile: string): Promise<NixEnvRecord> {
  const record = await backend.nix.select(folderPath, nixFile)
  detections.patch(folderPath, { selected: record })
  await refreshLspFolderEnvironment(folderPath)
  return record
}

export async function evaluateNix(folderPath: string): Promise<NixEnvRecord> {
  evaluating = new Set(evaluating).add(folderPath)
  try {
    const record = await backend.nix.evaluate(folderPath)
    detections.patch(folderPath, { selected: record })
    await refreshLspFolderEnvironment(folderPath)
    return record
  } finally {
    const next = new Set(evaluating)
    next.delete(folderPath)
    evaluating = next
  }
}

export async function clearNix(folderPath: string): Promise<void> {
  await backend.nix.clear(folderPath)
  detections.patch(folderPath, { selected: null })
  await refreshLspFolderEnvironment(folderPath)
}
