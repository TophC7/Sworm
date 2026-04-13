<!--
  @component
  CommitTooltip -- rich hover tooltip for a git graph commit row.

  Shows author, date, full message, diff stats, ref badges, and hash.
  Accepts eagerly-available GraphCommit data plus a lazily-loaded CommitDetail.
-->

<script lang="ts">
  import type { CommitDetail, GraphCommit } from '$lib/types/backend'
  import { timeAgo, formatFullDate } from '$lib/utils/date'
  import { refLabel, visibleRefs } from '$lib/utils/gitRefs'

  let {
    commit,
    detail,
    graphColor
  }: {
    commit: GraphCommit
    detail: CommitDetail | null
    graphColor: string
  } = $props()

  let copied = $state(false)

  // Aggregate diff stats from file list
  let stats = $derived.by(() => {
    if (!detail) return null
    let ins = 0
    let del = 0
    for (const f of detail.files) {
      ins += f.additions
      del += f.deletions
    }
    return { files: detail.files.length, insertions: ins, deletions: del }
  })

  // Parse body text into renderable lines
  let bodyLines = $derived.by(() => {
    const raw = detail?.body ?? ''
    if (!raw.trim()) return []
    return raw
      .split('\n')
      .filter((l) => l.trim())
      .map((line) => {
        const t = line.trim()
        if (t.startsWith('- ') || t.startsWith('* ')) {
          return { bullet: true as const, text: t.slice(2) }
        }
        return { bullet: false as const, text: t }
      })
  })

  let filteredRefs = $derived(visibleRefs(commit.refs))

  async function copyHash() {
    await navigator.clipboard.writeText(commit.hash)
    copied = true
    setTimeout(() => (copied = false), 1500)
  }
</script>

<div class="flex max-w-135 flex-col gap-2.5 py-0.5">
  <!-- Author + date -->
  <div class="flex items-center gap-1 text-[0.72rem]">
    <span class="text-success">&#x25CE;</span>
    <span class="font-semibold text-fg">{commit.author}</span>
    <span class="text-subtle">,</span>
    <span class="text-subtle">&#x25F7;</span>
    <span class="text-muted">{timeAgo(commit.date)}</span>
    <span class="text-subtle">({formatFullDate(commit.date)})</span>
  </div>

  <!-- Subject line -->
  <p class="text-[0.72rem] leading-snug text-fg">{commit.message}</p>

  <!-- Body (bullet list or paragraphs) -->
  {#if bodyLines.length > 0}
    <ul class="flex flex-col gap-0.5 pl-2 text-[0.68rem] leading-snug text-muted">
      {#each bodyLines as line}
        {#if line.bullet}
          <li class="flex gap-1.5">
            <span class="shrink-0 text-subtle">&bull;</span>
            <span>{line.text}</span>
          </li>
        {:else}
          <li class="list-none">{line.text}</li>
        {/if}
      {/each}
    </ul>
  {/if}

  <!-- Diff stats -->
  {#if stats}
    <p class="text-[0.68rem]">
      <span class="text-muted">{stats.files} files changed,</span>
      {' '}
      <span class="text-success">{stats.insertions} insertions(+)</span>
      <span class="text-muted">,</span>
      {' '}
      <span class="text-danger">{stats.deletions} deletions(-)</span>
    </p>
  {:else}
    <p class="text-[0.62rem] text-subtle">Loading stats...</p>
  {/if}

  <!-- Ref badges -->
  {#if filteredRefs.length > 0}
    <div class="flex flex-wrap gap-1.5">
      {#each filteredRefs as ref (ref)}
        <span
          class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 font-mono text-[0.62rem] leading-tight"
          style="border-color: {graphColor}40; color: {graphColor}"
        >
          <span class="text-[0.5rem]">&#x25CE;</span>
          {refLabel(ref)}
        </span>
      {/each}
    </div>
  {/if}

  <!-- Hash + copy -->
  <div class="flex items-center gap-1.5 text-[0.68rem]">
    <span class="text-subtle">&#x25C7;</span>
    <span class="font-mono text-accent">{commit.short_hash}</span>
    <button
      class="cursor-pointer text-subtle transition-colors hover:text-fg"
      onclick={copyHash}
      title="Copy full hash"
    >
      {#if copied}
        <svg class="size-3" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="2">
          <path d="M3 8.5l3 3 7-7" />
        </svg>
      {:else}
        <svg class="size-3" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5">
          <rect x="5.5" y="5.5" width="8" height="8" rx="1.5" />
          <path d="M10.5 5.5V3.5a1.5 1.5 0 00-1.5-1.5H3.5A1.5 1.5 0 002 3.5V9a1.5 1.5 0 001.5 1.5H5.5" />
        </svg>
      {/if}
    </button>
  </div>
</div>
