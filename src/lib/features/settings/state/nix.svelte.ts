// Per-folder Nix environment state using Svelte 5 runes.

import { backend } from '$lib/api/backend'
import { refreshLspFolderEnvironment } from '$lib/features/editor/lsp/registry'
import { createFolderKeyedStore } from '$lib/state/folderKeyedStore.svelte'
import type { NixDetection, NixEnvRecord } from '$lib/types/backend'

const detections = createFolderKeyedStore<NixDetection>()
let evaluating = $state<Set<string>>(new Set())
const folderGenerations = new Map<string, number>()

export function getNixDetection(folderPath: string): NixDetection | undefined {
  return detections.get(folderPath)
}

export function isNixEvaluating(folderPath: string): boolean {
  return evaluating.has(folderPath)
}

export async function detectNix(folderPath: string): Promise<NixDetection> {
  const generation = folderGenerations.get(folderPath) ?? 0
  const detection = await backend.nix.detect(folderPath)
  if ((folderGenerations.get(folderPath) ?? 0) === generation) detections.set(folderPath, detection)
  return detection
}

export async function selectNixFile(folderPath: string, nixFile: string): Promise<NixEnvRecord> {
  const generation = folderGenerations.get(folderPath) ?? 0
  const record = await backend.nix.select(folderPath, nixFile)
  if ((folderGenerations.get(folderPath) ?? 0) !== generation) return record
  detections.patch(folderPath, { selected: record })
  await refreshLspFolderEnvironment(folderPath)
  return record
}

export async function evaluateNix(folderPath: string): Promise<NixEnvRecord> {
  const generation = folderGenerations.get(folderPath) ?? 0
  evaluating = new Set(evaluating).add(folderPath)
  try {
    const record = await backend.nix.evaluate(folderPath)
    if ((folderGenerations.get(folderPath) ?? 0) !== generation) return record
    detections.patch(folderPath, { selected: record })
    await refreshLspFolderEnvironment(folderPath)
    return record
  } finally {
    if ((folderGenerations.get(folderPath) ?? 0) === generation) {
      const next = new Set(evaluating)
      next.delete(folderPath)
      evaluating = next
    }
  }
}

export async function clearNix(folderPath: string): Promise<void> {
  const generation = folderGenerations.get(folderPath) ?? 0
  await backend.nix.clear(folderPath)
  if ((folderGenerations.get(folderPath) ?? 0) !== generation) return
  detections.patch(folderPath, { selected: null })
  await refreshLspFolderEnvironment(folderPath)
}

/** Forget the folder's detection; called when the workbench releases the folder. */
export function releaseNixFolder(folderPath: string) {
  folderGenerations.set(folderPath, (folderGenerations.get(folderPath) ?? 0) + 1)
  detections.delete(folderPath)
  if (!evaluating.has(folderPath)) return
  const next = new Set(evaluating)
  next.delete(folderPath)
  evaluating = next
}
