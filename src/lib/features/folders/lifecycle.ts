// Cross-feature teardown for a folder that lost its last workbench tab.
// Every step is a no-op for folders that never populated its cache, and
// caches repopulate lazily when the folder is reopened.

import { backend } from '$lib/api/backend'
import { releaseLspFolder } from '$lib/features/editor/lsp/registry'
import { releaseFileTree } from '$lib/features/files/explorer.svelte'
import { releaseProjectFiles } from '$lib/features/files/projectFiles.svelte'
import { releaseBranchFolder } from '$lib/features/git/branches.svelte'
import { releaseGitFolder } from '$lib/features/git/state.svelte'
import { releaseWorktreeFolder } from '$lib/features/git/worktrees.svelte'
import { releaseIssueFolder } from '$lib/features/issues/state.svelte'
import { releaseProviderFolder } from '$lib/features/sessions/providers/state.svelte'
import { releaseNixFolder } from '$lib/features/settings/state/nix.svelte'

/** Shared teardown; destructive operations propagate shutdown/release failures. */
export async function releaseFolder(folderPath: string, strict = false): Promise<void> {
  releaseGitFolder(folderPath)
  releaseBranchFolder(folderPath)
  releaseWorktreeFolder(folderPath)
  releaseIssueFolder(folderPath)
  releaseNixFolder(folderPath)
  releaseProviderFolder(folderPath)
  releaseFileTree(folderPath)
  releaseProjectFiles(folderPath)
  if (strict) {
    await releaseLspFolder(folderPath, true)
    await backend.folders.release(folderPath)
  } else {
    void releaseLspFolder(folderPath)
    await backend.folders.release(folderPath).catch((error) => console.warn('Folder release failed:', error))
  }
}
