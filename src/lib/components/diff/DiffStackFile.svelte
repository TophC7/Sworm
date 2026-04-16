<!--
  @component
  Single file in a diff stack with expand/collapse header and lazy-loaded diff viewer.
  Handles file header rendering, status display, and editor actions.
-->

<script lang="ts">
  import { DiffMode } from '$lib/diff/types'
  import { gitStatusColor, gitStatusDisplay, gitStatusLabel } from '$lib/utils/gitStatus'
  import { openFile } from '$lib/utils/openFile'
  import { addReadonlyEditorTab } from '$lib/stores/workspace.svelte'
  import DiffViewer from '$lib/components/diff/DiffViewer.svelte'
  import LazyRender from '$lib/components/LazyRender.svelte'
  import FileIcon from '$lib/icons/FileIcon.svelte'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import { ChevronRight, SquareArrowOutUpRight, Eye } from '$lib/icons/lucideExports'

  interface Props {
    file: { path: string; status: string; additions: number; deletions: number }
    entry: { rawDiff: string; oldContent?: string | null; newContent?: string | null } | undefined
    expanded: boolean
    diffMode?: typeof DiffMode.Split | typeof DiffMode.Unified
    diffWrap?: boolean
    diffFontSize?: number
    loading?: boolean
    idPrefix?: string
    projectId?: string
    projectPath?: string
    commitHash?: string | null
    stashIndex?: number | null
    onToggle: (path: string) => void
  }

  let {
    file,
    entry,
    expanded,
    diffMode = DiffMode.Split,
    diffWrap = false,
    diffFontSize = 13,
    loading = false,
    idPrefix = 'diff',
    projectId = '',
    projectPath = '',
    commitHash = null,
    stashIndex = null,
    onToggle
  }: Props = $props()

  function openInEditor(filePath: string) {
    if (!projectId || !projectPath) return
    openFile(projectId, projectPath, filePath)
  }

  function viewAtCommit(filePath: string) {
    if (!projectId || !commitHash) return
    const short = commitHash.slice(0, 7)
    addReadonlyEditorTab(projectId, filePath, commitHash, short)
  }

  function viewAtStash(filePath: string) {
    if (!projectId || stashIndex == null) return
    const stashRef = `stash@{${stashIndex}}`
    addReadonlyEditorTab(projectId, filePath, stashRef, `stash-${stashIndex}`)
  }
</script>

{#snippet headerAction(Icon: typeof Eye, label: string, onclick: () => void)}
  <TooltipRoot delayDuration={300}>
    <TooltipTrigger class="rounded p-1 text-muted transition-colors hover:bg-accent/15 hover:text-fg" {onclick}>
      <Icon size={12} />
    </TooltipTrigger>
    <TooltipContent sideOffset={4}>{label}</TooltipContent>
  </TooltipRoot>
{/snippet}

<div id="{idPrefix}-{file.path}" class="border-b border-edge">
  <div class="sticky top-0 z-20 flex w-full items-center border-b border-edge/50 bg-raised/90 backdrop-blur-sm">
    <button
      class="flex min-w-0 flex-1 items-center gap-2 px-3 py-1.5 text-left transition-colors hover:bg-raised"
      onclick={() => onToggle(file.path)}
    >
      <ChevronRight size={12} class="shrink-0 text-muted transition-transform {expanded ? 'rotate-90' : ''}" />
      <span class="text-[0.65rem] font-bold {gitStatusColor(file.status)}" title={gitStatusLabel(file.status)}
        >{gitStatusDisplay(file.status)}</span
      >
      <FileIcon filename={file.path} size={13} />
      <span class="min-w-0 truncate font-mono text-[0.72rem] text-fg">{file.path}</span>
      <span class="ml-auto shrink-0 font-mono text-[0.62rem]">
        {#if file.additions > 0}<span class="text-success">+{file.additions}</span>{/if}
        {#if file.deletions > 0}<span class="ml-1 text-danger">-{file.deletions}</span>{/if}
      </span>
    </button>
    {#if projectId}
      <div class="flex shrink-0 items-center gap-0.5 pr-2">
        {#if commitHash}
          {@render headerAction(Eye, 'View at commit', () => viewAtCommit(file.path))}
          {#if file.status !== 'D'}
            {@render headerAction(SquareArrowOutUpRight, 'Open current file', () => openInEditor(file.path))}
          {/if}
        {:else if stashIndex != null}
          {@render headerAction(Eye, 'View in stash', () => viewAtStash(file.path))}
          {#if file.status !== 'D'}
            {@render headerAction(SquareArrowOutUpRight, 'Open current file', () => openInEditor(file.path))}
          {/if}
        {:else}
          {@render headerAction(SquareArrowOutUpRight, 'Open in editor', () => openInEditor(file.path))}
        {/if}
      </div>
    {/if}
  </div>

  {#if expanded}
    {#if loading}
      <div class="px-4 py-6 text-center text-[0.72rem] text-subtle">Loading diff...</div>
    {:else if entry}
      <LazyRender minHeight={Math.min(Math.max(80, (file.additions + file.deletions) * 22), 600)}>
        <DiffViewer
          rawDiff={entry.rawDiff}
          filePath={file.path}
          oldContent={entry.oldContent}
          newContent={entry.newContent}
          mode={diffMode}
          wrap={diffWrap}
          fontSize={diffFontSize}
        />
      </LazyRender>
    {:else}
      <div class="px-4 py-6 text-center text-[0.72rem] text-subtle">No diff available</div>
    {/if}
  {/if}
</div>
