// Client-side branch-name validation. A subset of
// `git check-ref-format`; treat as best-effort since the server-side
// `validated_ref_name` plus `git`'s own check are the final authority.

const FORBIDDEN_CHARS = /[\s~^:?*[\\\x00-\x1f\x7f]/

export function validateBranchName(name: string): string | null {
  if (!name) return 'Name is required'
  if (name.startsWith('-')) return "Name can't start with '-'"
  if (name.startsWith('/') || name.endsWith('/')) return "Name can't start or end with '/'"
  if (name.endsWith('.lock')) return "Name can't end with '.lock'"
  if (name.includes('..')) return "Name can't contain '..'"
  if (name.includes('@{')) return "Name can't contain '@{'"
  if (FORBIDDEN_CHARS.test(name)) return 'Name has illegal characters'
  for (const segment of name.split('/')) {
    if (!segment) return "Name can't have consecutive '/'"
    if (segment.startsWith('.')) return "Segment can't start with '.'"
    if (segment.endsWith('.')) return "Segment can't end with '.'"
  }
  return null
}
