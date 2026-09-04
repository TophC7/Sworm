import { convertFileSrc } from '@tauri-apps/api/core'
import { dirname, normalizeAbsolutePath } from '$lib/utils/paths'

const URL_SCHEME_RE = /^[a-zA-Z][a-zA-Z0-9+.-]*:/

export function mediaAssetUrl(folderPath: string, filePath: string): string {
  return convertFileSrc(normalizeAbsolutePath(`${folderPath}/${filePath}`))
}

// Markdown image URLs are document-relative. Browser-relative URLs point at Vite/Tauri chrome, not the repo.
export function markdownImageSrc(
  href: string | null | undefined,
  folderPath?: string,
  markdownPath?: string | null
): string {
  if (!href) return ''
  const trimmed = href.trim()
  if (!trimmed || URL_SCHEME_RE.test(trimmed) || trimmed.startsWith('//')) return trimmed
  if (!folderPath || !markdownPath) return trimmed

  const localPath = resolveMarkdownLocalPath(markdownPath, trimmed)
  return localPath ? mediaAssetUrl(folderPath, localPath) : trimmed
}

export function resolveMarkdownLocalPath(markdownPath: string, href: string): string | null {
  const [pathPart] = href.split(/[?#]/, 1)
  if (!pathPart) return null

  const decoded = decodeHrefPath(pathPart).replaceAll('\\', '/')
  const baseParts = decoded.startsWith('/') ? [] : dirname(markdownPath).split('/').filter(Boolean)

  for (const part of decoded.split('/')) {
    if (!part || part === '.') continue
    if (part === '..') {
      if (baseParts.length === 0) return null
      baseParts.pop()
      continue
    }
    baseParts.push(part)
  }

  return baseParts.join('/')
}

function decodeHrefPath(value: string): string {
  try {
    return decodeURIComponent(value)
  } catch {
    return value
  }
}
