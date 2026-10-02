import { normalizeAbsolutePath, splitRemotePath } from './paths'

/**
 * MurmurHash3 32-bit finalizer mix.
 * Provides high avalanche effect so minor path variations (e.g. project-1 vs project-2)
 * disperse uniformly across the entire color spectrum.
 */
function fmix32(h: number): number {
  h ^= h >>> 16
  h = Math.imul(h, 0x85ebca6b)
  h ^= h >>> 13
  h = Math.imul(h, 0xc2b2ae35)
  h ^= h >>> 16
  return h >>> 0
}

/**
 * Deterministic 32-bit hash for string paths.
 * Combines FNV-1a with fmix32 avalanche mixing.
 */
function hashPath(str: string): number {
  let hash = 0x811c9dc5
  for (let i = 0; i < str.length; i++) {
    hash ^= str.charCodeAt(i)
    hash = Math.imul(hash, 0x01000193)
  }
  return fmix32(hash)
}

const colorCache = new Map<string, string>()
const MAX_CACHE_ENTRIES = 256

/**
 * Deterministic CSS color for a project path. Hashes the server-local path, so
 * a remote folder keeps its color whether reached as `sworm://<server>/...`
 * from desktop or as a plain path from that server's web client.
 */
export function getPathColor(path: string): string {
  const cached = colorCache.get(path)
  if (cached) return cached

  const normalized = normalizeAbsolutePath(path || '/')
  const hash = hashPath(splitRemotePath(normalized)?.path ?? normalized)
  // Mix each axis separately so nearby hues need not share lightness or chroma.
  const hue = (hash / 0xffffffff) * 360
  const lightness = 0.65 + (fmix32(hash ^ 0x9e3779b9) / 0xffffffff) * 0.15
  const chroma = 0.1 + (fmix32(hash ^ 0x243f6a88) / 0xffffffff) * 0.08
  // Keep colors bright and colorful; native CSS handles display gamut mapping.
  const color = `oklch(${lightness.toFixed(4)} ${chroma.toFixed(4)} ${hue.toFixed(2)})`

  if (colorCache.size >= MAX_CACHE_ENTRIES) colorCache.clear()
  colorCache.set(path, color)
  return color
}
