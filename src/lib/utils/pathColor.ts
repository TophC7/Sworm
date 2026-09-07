import { normalizeAbsolutePath } from './paths'

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

/**
 * Returns a deterministic hue in [0, 359] for the given folder path.
 * Normalizes absolute path before hashing so trailing slashes yield identical hues.
 */
function getPathHue(path: string): number {
  const normalized = normalizeAbsolutePath(path || '/')
  return hashPath(normalized) % 360
}

const colorCache = new Map<string, string>()
const MAX_CACHE_ENTRIES = 256

/** Returns a deterministic CSS color string for the given project path. */
export function getPathColor(path: string): string {
  const cached = colorCache.get(path)
  if (cached) return cached

  const hue = getPathHue(path)
  // Blues and violets have slightly lower perceived luminance in sRGB;
  // bump lightness slightly so visual contrast against dark surfaces stays balanced.
  const lightness = hue >= 210 && hue <= 290 ? 70 : 65
  const color = `hsl(${hue}, 75%, ${lightness}%)`

  if (colorCache.size >= MAX_CACHE_ENTRIES) colorCache.clear()
  colorCache.set(path, color)
  return color
}
