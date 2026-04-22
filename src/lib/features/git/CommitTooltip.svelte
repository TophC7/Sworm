<!--
  @component
  CommitTooltip -- rich hover tooltip for a git graph commit row.

  Shows author, date, full message, diff stats, ref badges, and hash.
  Accepts eagerly-available GraphCommit data plus a lazily-loaded CommitDetail.
-->

<script lang="ts">
  import type { CommitDetail, GraphCommit } from '$lib/types/backend'
  import { timeAgo, formatFullDate } from '$lib/utils/date'
  import { refLabel, visibleRefs } from '$lib/features/git/gitRefs'
  import { Check, ClockIcon, CopyIcon, GitCommitIcon } from '$lib/icons/lucideExports'
  import { IconButton } from '$lib/components/ui/button'

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
  <div class="flex items-center gap-1 text-sm">
    <span class="inline-block h-1.5 w-1.5 shrink-0 rounded-full bg-success" aria-hidden="true"></span>
    <span class="font-semibold text-fg">{commit.author}</span>
    <span class="text-subtle">,</span>
    <ClockIcon size={10} class="text-subtle" />
    <span class="text-muted">{timeAgo(commit.date)}</span>
    <span class="text-subtle">({formatFullDate(commit.date)})</span>
  </div>

  <!-- Subject line -->
  <p class="text-sm leading-snug text-fg">{commit.message}</p>

  <!-- Body (bullet list or paragraphs) -->
  {#if bodyLines.length > 0}
    <ul class="flex flex-col gap-0.5 pl-2 text-xs leading-snug text-muted">
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
    <p class="text-xs">
      <span class="text-muted">{stats.files} files changed,</span>
      {' '}
      <span class="text-success">{stats.insertions} insertions(+)</span>
      <span class="text-muted">,</span>
      {' '}
      <span class="text-danger">{stats.deletions} deletions(-)</span>
    </p>
  {:else}
    <p class="text-2xs text-subtle">Loading stats...</p>
  {/if}

  <!-- Ref badges -->
  {#if filteredRefs.length > 0}
    <div class="flex flex-wrap gap-1.5">
      {#each filteredRefs as ref (ref)}
        <span
          class="inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 font-mono text-2xs leading-tight"
          style="border-color: {graphColor}40; color: {graphColor}"
        >
          <span class="inline-block h-1 w-1 shrink-0 rounded-full" style="background: {graphColor}" aria-hidden="true"
          ></span>
          {refLabel(ref)}
        </span>
      {/each}
    </div>
  {/if}

  <!-- Hash + copy -->
  <div class="flex items-center gap-1.5 text-xs">
    <GitCommitIcon size={10} class="text-subtle" />
    <span class="font-mono text-accent">{commit.short_hash}</span>
    <IconButton ariaLabel="Copy full hash" onclick={copyHash}>
      {#if copied}
        <Check size={12} class="text-success" />
      {:else}
        <CopyIcon size={12} />
      {/if}
    </IconButton>
  </div>
</div>
