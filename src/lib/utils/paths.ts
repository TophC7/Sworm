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

export function isEqualOrParent(parentPath: string, childPath: string): boolean {
  const parent = normalizeRelativePath(parentPath)
  const child = normalizeRelativePath(childPath)
  if (parent === child) return true
  if (parent === '') return child.length > 0
  return child.startsWith(`${parent}/`)
}

export function toProjectRelativePath(projectPath: string, absolutePath: string): string | null {
  const root = normalizeAbsolutePath(projectPath)
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

/** Shorten an absolute path's parent directory, replacing the home prefix with ~. */
export function parentPath(path: string): string {
  const parts = path.split('/')
  parts.pop()
  const parent = parts.join('/')
  let home = ''
  if (parts[1] === 'home' && parts[2]) {
    home = `/home/${parts[2]}`
  } else if (parts[1] === 'root') {
    home = '/root'
  }
  return home && parent.startsWith(home) ? '~' + parent.slice(home.length) : parent
}
