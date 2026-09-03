// Cross-feature teardown for a folder that lost its last workbench tab.
// Every step is a no-op for folders that never populated its cache, and
// caches repopulate lazily when the folder is reopened.

import { backend } from '$lib/api/backend'
import { releaseLspFolder } from '$lib/features/editor/lsp/registry'
import { releaseFileTree } from '$lib/features/files/fileTree.svelte'
import { releaseProjectFiles } from '$lib/features/files/projectFiles.svelte'
import { releaseBranchFolder } from '$lib/features/git/branches.svelte'
import { releaseGitFolder } from '$lib/features/git/state.svelte'
import { releaseProviderFolder } from '$lib/features/sessions/providers/state.svelte'
import { releaseNixFolder } from '$lib/features/settings/state/nix.svelte'

/** Evict every folder-scoped cache and stop the folder's backend services (issue bridge, issue DB). */
export function releaseFolder(folderPath: string): void {
  releaseGitFolder(folderPath)
  releaseBranchFolder(folderPath)
  releaseNixFolder(folderPath)
  releaseProviderFolder(folderPath)
  releaseFileTree(folderPath)
  releaseProjectFiles(folderPath)
  void releaseLspFolder(folderPath)
  void backend.folders.release(folderPath).catch((error) => console.warn('Folder release failed:', error))
}
