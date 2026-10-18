import { SvelteMap } from 'svelte/reactivity'
import { nextRevealSeq, type ExplorerReveal } from '$lib/features/files/explorer.svelte'

const reveals = new SvelteMap<string, ExplorerReveal>()

export function getGitTreeReveal(folderPath: string): ExplorerReveal | null {
  return reveals.get(folderPath) ?? null
}

export function requestGitTreeReveal(folderPath: string, path: string, activation: number): void {
  reveals.set(folderPath, { path, seq: nextRevealSeq(), activation })
}

export function releaseGitTreeReveal(folderPath: string): void {
  reveals.delete(folderPath)
}
