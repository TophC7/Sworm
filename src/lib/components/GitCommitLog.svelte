<script lang="ts">
  import type { GitCommit } from '$lib/types/backend'
  import { backend } from '$lib/api/backend'

  let { projectPath }: { projectPath: string } = $props()

  let commits = $state<GitCommit[]>([])
  let currentPath = ''

  $effect(() => {
    const path = projectPath
    currentPath = path
    void loadCommits(path)
  })

  async function loadCommits(path: string) {
    try {
      const result = await backend.git.getLog(path, 20)
      // Guard against stale results from rapid project switching
      if (path === currentPath) commits = result
    } catch {
      if (path === currentPath) commits = []
    }
  }
</script>

<div class="text-[0.78rem]">
  <div class="flex items-center justify-between px-2.5 py-1.5">
    <span class="text-[0.7rem] font-semibold tracking-wide text-muted uppercase">Commits</span>
  </div>

  {#if commits.length === 0}
    <div class="px-2.5 py-2 text-[0.75rem] text-subtle">No recent commits.</div>
  {:else}
    {#each commits as commit (commit.hash)}
      <div class="border-t border-edge/45 px-2.5 py-2">
        <div class="mb-1 flex items-center justify-between gap-2.5">
          <span class="font-mono text-[0.72rem] text-accent">{commit.short_hash}</span>
          <span class="text-[0.68rem] text-muted">{commit.date}</span>
        </div>
        <div class="text-[0.76rem] text-fg">{commit.message}</div>
        <div class="text-[0.68rem] text-muted">{commit.author}</div>
      </div>
    {/each}
  {/if}
</div>
