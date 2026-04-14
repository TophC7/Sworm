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
