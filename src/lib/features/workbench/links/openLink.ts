import { backend } from '$lib/api/backend'
import { notify } from '$lib/features/notifications/state.svelte'
import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
import { basename, dirname, normalizeRelativePath, toProjectRelativePath } from '$lib/utils/paths'
import { revealPath } from '$lib/features/files/fileTree.svelte'
import { openFolder } from '$lib/features/workbench/state.svelte'
import { openTextFile, type TextRevealTarget } from '$lib/features/workbench/surfaces/text/service.svelte'
import { openUrl } from '@tauri-apps/plugin-opener'
import { parseFilePathAndLocation, parseLinkTarget, type ParsedLinkTarget } from './parseLink'
const GITHUB_REMOTE_REGEX = /(?:github\.com[:/])([^/\s]+)\/([^/\s]+?)(?:\.git)?(?:\s|$)/
const MAX_GIT_CACHE_ENTRIES = 16
const gitRepoCache = new Map<string, { owner: string; repo: string } | null>()

const OMP_SCHEMES: Record<string, true> = {
  omp: true,
  skill: true,
  rule: true,
  local: true,
  artifact: true,
  history: true,
  agent: true
}
async function getGitHubRepo(folderPath: string): Promise<{ owner: string; repo: string } | null> {
  if (gitRepoCache.has(folderPath)) {
    return gitRepoCache.get(folderPath) ?? null
  }

  try {
    const config = await backend.files.read(folderPath, '.git/config')
    const match = config.match(GITHUB_REMOTE_REGEX)
    const result = match ? { owner: match[1], repo: match[2] } : null
    if (gitRepoCache.size >= MAX_GIT_CACHE_ENTRIES) gitRepoCache.clear()
    gitRepoCache.set(folderPath, result)
    return result
  } catch {
    // Not a git repo or unable to read .git/config
    if (gitRepoCache.size >= MAX_GIT_CACHE_ENTRIES) gitRepoCache.clear()
    gitRepoCache.set(folderPath, null)
    return null
  }
}

/**
 * Open a link from terminal output or markdown view.
 *
 * @param rawTarget The clicked link URL, URI, or file path.
 * @param folderPath Active workbench folder path.
 */
export async function openLink(rawTarget: string, folderPath?: string | null): Promise<boolean> {
  const target = parseLinkTarget(rawTarget)

  try {
    if (target.kind === 'anchor') {
      // Fragment-only link (#section); handled within document or no-op
      return true
    }

    if (target.kind === 'web' && target.url) {
      await openUrl(target.url)
      return true
    }

    if (target.kind === 'uri') {
      return await handleUriLink(target, folderPath)
    }

    if (target.kind === 'file') {
      return await handleFileLink(target.path ?? rawTarget, target.reveal, folderPath)
    }
  } catch (error) {
    notify.error('Failed to open link', getErrorMessage(error))
    return false
  }

  return false
}

async function handleUriLink(target: ParsedLinkTarget, folderPath?: string | null): Promise<boolean> {
  const scheme = target.scheme
  const fullUrl = target.url ?? target.raw

  if (scheme === 'file') {
    return handleFileScheme(fullUrl, folderPath)
  }

  if (scheme === 'issue' || scheme === 'pr') {
    return handleIssueOrPrScheme(scheme, fullUrl, folderPath)
  }

  if (scheme && OMP_SCHEMES[scheme]) {
    return handleOmpScheme(fullUrl, target.reveal, folderPath)
  }

  // Fallback: try opening with system opener
  try {
    await openUrl(fullUrl)
    return true
  } catch {
    notify.error('Unsupported link', `Scheme "${scheme}://" is not supported.`)
    return false
  }
}

async function handleOmpScheme(
  fullUrl: string,
  reveal?: TextRevealTarget | null,
  folderPath?: string | null
): Promise<boolean> {
  try {
    const resolved = await backend.sessions.ompResolveUri(fullUrl, folderPath)
    if (resolved.is_dir) {
      await openFolder(resolved.path)
      return true
    }
    const dir = dirname(resolved.path)
    const file = basename(resolved.path)
    if (dir && file) {
      await openTextFile(dir, file, { temporary: false, reveal })
      return true
    }
  } catch (err) {
    notify.error('Cannot open link', getErrorMessage(err))
    return false
  }
  return false
}

async function handleFileScheme(fullUrl: string, folderPath?: string | null): Promise<boolean> {
  let parsedUrl: URL
  try {
    parsedUrl = new URL(fullUrl)
  } catch {
    const rawPath = fullUrl.replace(/^file:\/\//, '')
    return handleFileLink(rawPath, null, folderPath)
  }

  let absPath = decodeURIComponent(parsedUrl.pathname)
  let reveal: TextRevealTarget | null = null
  if (parsedUrl.hash) {
    const { reveal: hashReveal } = parseFilePathAndLocation(`dummy${parsedUrl.hash}`)
    reveal = hashReveal
  }

  if (!reveal) {
    const parsed = parseFilePathAndLocation(absPath)
    absPath = parsed.filePath
    reveal = parsed.reveal
  }

  return handleFileLink(absPath, reveal, folderPath)
}

async function handleIssueOrPrScheme(
  scheme: 'issue' | 'pr',
  fullUrl: string,
  folderPath?: string | null
): Promise<boolean> {
  const match = fullUrl.match(/^(?:issue|pr):\/\/(?:([^/]+)\/([^/]+)\/)?(\d+)/)
  if (!match) {
    notify.error('Invalid link', `Cannot parse ${fullUrl}`)
    return false
  }

  let owner = match[1]
  let repo = match[2]
  const id = match[3]
  const section = scheme === 'issue' ? 'issues' : 'pull'

  if (!owner || !repo) {
    if (folderPath) {
      const git = await getGitHubRepo(folderPath)
      if (git) {
        owner = git.owner
        repo = git.repo
      }
    }
  }

  if (!owner || !repo) {
    notify.error('Cannot open link', `No repository identified for ${scheme} #${id}`)
    return false
  }

  const githubUrl = `https://github.com/${owner}/${repo}/${section}/${id}`
  await openUrl(githubUrl)
  return true
}

async function handleFileLink(
  filePath: string,
  reveal?: TextRevealTarget | null,
  folderPath?: string | null
): Promise<boolean> {
  let targetPath = filePath
  if (targetPath.startsWith('file://')) {
    try {
      targetPath = decodeURIComponent(new URL(targetPath).pathname)
    } catch {
      targetPath = targetPath.slice(7)
    }
  }

  // Expand ~ if folderPath is in /home or /root
  if (targetPath.startsWith('~/') && folderPath) {
    const parts = folderPath.split('/')
    const home = parts[1] === 'home' && parts[2] ? `/home/${parts[2]}` : parts[1] === 'root' ? '/root' : null
    if (home) {
      targetPath = `${home}/${targetPath.slice(2)}`
    }
  }

  // Compute absolute path if possible
  const absPath = targetPath.startsWith('/')
    ? targetPath
    : folderPath
      ? `${folderPath}/${normalizeRelativePath(targetPath)}`
      : null

  // Check if target is an existing directory
  if (absPath) {
    try {
      const resolved = await backend.folders.resolve(absPath)
      // Subdirectory of current project folder: reveal in files tree
      if (folderPath && absPath.startsWith(folderPath + '/') && absPath !== folderPath) {
        const rel = toProjectRelativePath(folderPath, absPath)
        if (rel) {
          await revealPath(folderPath, rel)
          return true
        }
      }
      // External directory or project root: open as folder tab
      await openFolder(resolved.path)
      return true
    } catch {
      // Not a directory or does not exist as directory, proceed to open as file
    }
  }

  // File inside current project
  if (folderPath) {
    const projectRel = toProjectRelativePath(folderPath, targetPath)
    const rel = projectRel ?? (!targetPath.startsWith('/') ? normalizeRelativePath(targetPath) : null)

    if (rel !== null) {
      try {
        await openTextFile(folderPath, rel, {
          temporary: false,
          reveal
        })
        return true
      } catch (e) {
        notify.error('Cannot open file', getErrorMessage(e))
        return false
      }
    }
  }

  // Absolute file path outside project folder: open directly in Monaco
  if (targetPath.startsWith('/')) {
    const dir = dirname(targetPath)
    const file = basename(targetPath)
    if (dir && file) {
      try {
        await openTextFile(dir, file, {
          temporary: false,
          reveal
        })
        return true
      } catch (e) {
        notify.error('Cannot open file', getErrorMessage(e))
        return false
      }
    }
  }

  notify.error('Cannot open file', `No active project folder to resolve relative path "${filePath}"`)
  return false
}
