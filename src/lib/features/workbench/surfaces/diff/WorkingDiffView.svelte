<script lang="ts">
  // Working-tree (staged or unstaged) diff viewer.
  import { backend } from '$lib/api/backend'
  import type { FileDiff } from '$lib/types/backend'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import DiffStack from '$lib/features/workbench/surfaces/diff/DiffStack.svelte'
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

  // Summary drives the reload trigger; the file list itself comes from
  // the unified `diff_get_files` endpoint so every caller sees the same shape.
  let summary = $derived(getGitSummary(projectId))
  let changeSignature = $derived(
    (summary?.changes ?? [])
      .filter((c) => c.staged === staged)
      .filter((c) => !scopePath || c.path === scopePath || c.path.startsWith(scopePath + '/'))
      .map((c) => `${c.path}:${c.status}:${c.additions}:${c.deletions}`)
      .join('\0')
  )

  let files = $state<FileDiff[]>([])
  const loader = createTrackedAsyncLoad<string>()
  let loading = $derived(loader.loading)

  $effect(() => {
    const sig = changeSignature
    loader.run(sig, async (isCurrent) => {
      const allFiles = await backend.git.getDiffFiles(projectPath, { kind: 'working', staged })
      if (!isCurrent()) return
      files = scopePath
        ? allFiles.filter((file) => file.path === scopePath || file.path.startsWith(scopePath + '/'))
        : allFiles
    })
  })
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
/>
