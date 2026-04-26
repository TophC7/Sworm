<script lang="ts">
  // Working-tree (staged or unstaged) diff viewer.
  //
  // Lazy content flow: the initial fetch pulls the cheap *index* (file
  // list + metadata, no content) so even a 200-file working tree
  // returns in tens of milliseconds. Per-file content is loaded on
  // demand by `DiffModelStore`'s fetcher as rows are retained.
  import { backend } from '$lib/api/backend'
  import type { FileDiff } from '$lib/types/backend'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import DiffStack from '$lib/features/workbench/surfaces/diff/DiffStack.svelte'
  import type { DiffContentFetcher } from '$lib/features/workbench/surfaces/diff/diffModels.svelte'
  import { createTrackedAsyncLoad } from '$lib/utils/trackedAsyncLoad.svelte'

  let {
    projectId,
    projectPath,
    staged,
    scopePath = null,
    revealNonce = 0,
    initialFile = null
  }: {
    projectId: string
    projectPath: string
    staged: boolean
    scopePath?: string | null
    revealNonce?: number
    initialFile?: string | null
  } = $props()

  // Summary drives the reload trigger; the index comes from the
  // working-tree endpoint that returns metadata only.
  let summary = $derived(getGitSummary(projectId))
  let changeSignature = $derived(
    (summary?.changes ?? [])
      .filter((c) => c.staged === staged)
      .filter((c) => !scopePath || c.path === scopePath || c.path.startsWith(scopePath + '/'))
      .map((c) => `${c.path}:${c.status}:${c.additions}:${c.deletions}`)
      .join('\0')
  )

  let files = $state<FileDiff[]>([])
  const indexLoad = createTrackedAsyncLoad<string>()
  let loading = $derived(indexLoad.loading)

  $effect(() => {
    // Fold every input that affects the result into the key so
    // a prop flip (project switch, staged toggle, scope change) always
    // re-runs even when changeSignature happens to collide.
    const key = `${projectPath}|${staged}|${scopePath ?? ''}|${changeSignature}`
    indexLoad.run(key, async (isCurrent) => {
      const indexFiles = await backend.git.getWorkingDiffIndex(projectPath, staged)
      if (!isCurrent()) return
      files = scopePath
        ? indexFiles.filter((file) => file.path === scopePath || file.path.startsWith(scopePath + '/'))
        : indexFiles
    })
  })

  // Content fetcher used by DiffModelStore. Each row asks for its own
  // content the first time it's retained. Most large diffs have only
  // a handful of rows scrolled into view at a time, so the total bytes
  // pulled stays a tiny fraction of the eager-payload version.
  const contentFetcher: DiffContentFetcher = async (entry) => {
    return await backend.git.getWorkingDiffFile(projectPath, entry.path, entry.status, staged)
  }
</script>

<DiffStack
  {files}
  {loading}
  {initialFile}
  scrollNonce={revealNonce}
  label={scopePath ? scopePath : staged ? 'Staged' : 'Changes'}
  idPrefix="changes-file"
  {projectId}
  {projectPath}
  workingStaged={staged}
  {contentFetcher}
/>
