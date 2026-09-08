function normalizeSlashes(path: string): string {
  return path.replaceAll('\\', '/')
}

export function normalizeRelativePath(path: string): string {
  const cleaned = normalizeSlashes(path)
    .replace(/^\.\/+/, '')
    .replace(/\/+/g, '/')
  return cleaned.replace(/\/$/, '')
}

export function normalizeAbsolutePath(path: string): string {
  const cleaned = normalizeSlashes(path).replace(/\/+/g, '/')
  if (cleaned === '/') return cleaned
  return cleaned.replace(/\/$/, '')
}

export function resolveProjectFile(folderPath: string, filePath: string): string {
  return normalizeAbsolutePath(`${folderPath}/${filePath}`)
}

export function isEqualOrParent(parentPath: string, childPath: string): boolean {
  const parent = normalizeRelativePath(parentPath)
  const child = normalizeRelativePath(childPath)
  if (parent === child) return true
  if (parent === '') return child.length > 0
  return child.startsWith(`${parent}/`)
}

export function toProjectRelativePath(folderPath: string, absolutePath: string): string | null {
  const root = normalizeAbsolutePath(folderPath)
  const absolute = normalizeAbsolutePath(absolutePath)
  if (absolute === root) return ''
  if (!absolute.startsWith(`${root}/`)) return null
  const relative = absolute.slice(root.length + 1)
  return normalizeRelativePath(relative)
}

export function basename(path: string): string {
  const normalized = normalizeSlashes(path)
  const index = normalized.lastIndexOf('/')
  return index === -1 ? normalized : normalized.slice(index + 1)
}

export function dirname(path: string): string {
  const normalized = normalizeRelativePath(path)
  const index = normalized.lastIndexOf('/')
  return index === -1 ? '' : normalized.slice(0, index)
}

function homePrefix(path: string): string {
  return path.match(/^\/(?:home\/[^/]+|root)(?=\/|$)/)?.[0] ?? ''
}

/** Absolute ancestor paths, collapsing the conventional home prefix to ~. */
export function pathCrumbs(path: string): Array<{ label: string; path: string }> {
  const normalized = normalizeAbsolutePath(path)
  const home = homePrefix(normalized)
  const crumbs = [{ label: home ? '~' : '/', path: home || '/' }]
  let ancestor = home
  for (const part of normalized.slice(home.length).split('/').filter(Boolean)) {
    ancestor += `/${part}`
    crumbs.push({ label: part, path: ancestor })
  }
  return crumbs
}

/** Shorten an absolute path's parent directory, replacing the home prefix with ~. */
export function parentPath(path: string): string {
  const parent = dirname(path)
  const home = homePrefix(parent)
  return home ? '~' + parent.slice(home.length) : parent
}

/** Last three path components joined with › for the status-bar breadcrumb. */
export function folderCrumbs(path: string): string {
  return normalizeSlashes(path).split('/').filter(Boolean).slice(-3).join(' › ')
}
