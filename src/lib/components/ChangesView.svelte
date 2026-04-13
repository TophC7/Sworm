<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { getGitSummary } from '$lib/stores/git.svelte'
  import DiffStack, { type DiffEntry, type DiffFile } from '$lib/components/DiffStack.svelte'

  let {
    projectId,
    projectPath,
    staged,
    initialFile = null
  }: {
    projectId: string
    projectPath: string
    staged: boolean
    initialFile?: string | null
  } = $props()

  let summary = $derived(getGitSummary(projectId))
  let changes = $derived(summary?.changes.filter((c) => c.staged === staged) ?? [])
  let files: DiffFile[] = $derived(
    changes.map((c) => ({
      path: c.path,
      status: c.status,
      additions: c.additions ?? 0,
      deletions: c.deletions ?? 0
    }))
  )
  // Untracked file paths for the batch endpoint (git diff won't include them)
  let untrackedPaths = $derived(changes.filter((c) => c.status === '?').map((c) => c.path))

  let rawDiffs = $state<Record<string, string>>({})
  let loadingDiffs = $state(false)

  let diffs = $derived.by(() => {
    const m = new Map<string, DiffEntry>()
    for (const [p, d] of Object.entries(rawDiffs)) {
      m.set(p, { rawDiff: d })
    }
    return m
  })

  // Reload when file list or stats change
  let lastFileKey = ''
  $effect(() => {
    const key = files.map((f) => `${f.path}:${f.additions}:${f.deletions}`).join('\0')
    if (key === lastFileKey) return
    lastFileKey = key
    void loadDiffs()
  })

  async function loadDiffs() {
    if (files.length === 0) {
      rawDiffs = {}
      return
    }

    loadingDiffs = true
    try {
      rawDiffs = await backend.git.getWorkingDiffs(projectPath, staged, untrackedPaths)
    } catch {
      rawDiffs = {}
    } finally {
      loadingDiffs = false
    }
  }
</script>

<DiffStack
  {files}
  {diffs}
  loading={loadingDiffs}
  {initialFile}
  label={staged ? 'Staged' : 'Changes'}
  idPrefix="changes-file"
/>
