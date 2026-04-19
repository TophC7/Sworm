<!--
  @component
  Hunk separator row with expand controls.

  Always renders when a hunk exists at this index. Expand buttons
  only appear when diffFile.getExpandEnabled() is true and there
  are hidden lines to reveal.
-->

<script lang="ts">
  import { type DiffFile, DiffMode, composeLen } from '$lib/diff/types'
  import { lineHeight } from '$lib/diff/layout'
  import { IconButton } from '$lib/components/ui/button'
  import { ChevronUp, ChevronDown, ChevronsUpDown } from '$lib/icons/lucideExports'

  let {
    diffFile,
    index,
    mode,
    fontSize
  }: {
    diffFile: DiffFile
    index: number
    mode: DiffMode
    fontSize: number
  } = $props()

  const getHunk = () =>
    mode === DiffMode.Unified ? diffFile.getUnifiedHunkLine(index) : diffFile.getSplitHunkLine(index)

  const getInfo = () => {
    const h = getHunk()
    return mode === DiffMode.Unified ? h?.unifiedInfo : h?.splitInfo
  }

  let hunk = $state(getHunk())
  let hasHiddenLines = $state(false)
  let showExpandAll = $state(false)

  const refresh = () => {
    hunk = getHunk()
    const info = getInfo()
    hasHiddenLines = info != null && info.startHiddenIndex < info.endHiddenIndex
    showExpandAll = info != null && info.endHiddenIndex - info.startHiddenIndex < composeLen
  }

  $effect(() => {
    refresh()
    return diffFile.subscribe(refresh)
  })

  let hunkText = $derived.by(() => {
    if (!hunk) return ''
    const info = mode === DiffMode.Unified ? hunk.unifiedInfo : hunk.splitInfo
    return info?.plainText ?? hunk.text ?? ''
  })

  let isFirst = $derived(hunk?.isFirst ?? false)
  let isLast = $derived(hunk?.isLast ?? false)
  let canExpand = $derived(diffFile.getExpandEnabled() && hasHiddenLines)

  function expand(dir: 'up' | 'down' | 'all') {
    if (mode === DiffMode.Unified) {
      diffFile.onUnifiedHunkExpand(dir, index)
    } else {
      diffFile.onSplitHunkExpand(dir, index)
    }
  }
</script>

{#if hunk}
  <div
    class="flex items-center bg-accent/6 font-mono text-muted select-none"
    style="font-size: {fontSize}px; line-height: {lineHeight(fontSize)};"
  >
    <!-- Sticky expand controls: stay visible during horizontal scroll -->
    <div class="sticky left-0 z-10 shrink-0" style="min-width: 40px; background-color: var(--color-ground);">
      <div class="flex h-full items-center justify-center gap-0.5 bg-accent/6 px-1">
        {#if canExpand}
          {#if isFirst}
            <IconButton
              tooltip="Expand down"
              tooltipSide="right"
              class="rounded p-0.5 text-muted transition-colors hover:bg-accent/15 hover:text-fg"
              onclick={() => expand('down')}
            >
              <ChevronDown size={12} />
            </IconButton>
          {:else if isLast}
            <IconButton
              tooltip="Expand up"
              tooltipSide="right"
              class="rounded p-0.5 text-muted transition-colors hover:bg-accent/15 hover:text-fg"
              onclick={() => expand('up')}
            >
              <ChevronUp size={12} />
            </IconButton>
          {:else if showExpandAll}
            <IconButton
              tooltip="Expand all"
              tooltipSide="right"
              class="rounded p-0.5 text-muted transition-colors hover:bg-accent/15 hover:text-fg"
              onclick={() => expand('all')}
            >
              <ChevronsUpDown size={12} />
            </IconButton>
          {:else}
            <div class="flex flex-col">
              <IconButton
                tooltip="Expand down"
                tooltipSide="right"
                class="rounded p-0.5 text-muted transition-colors hover:bg-accent/15 hover:text-fg"
                onclick={() => expand('down')}
              >
                <ChevronDown size={10} />
              </IconButton>
              <IconButton
                tooltip="Expand up"
                tooltipSide="right"
                class="rounded p-0.5 text-muted transition-colors hover:bg-accent/15 hover:text-fg"
                onclick={() => expand('up')}
              >
                <ChevronUp size={10} />
              </IconButton>
            </div>
          {/if}
        {/if}
      </div>
    </div>
    <span class="truncate pr-2 text-xs text-subtle">{hunkText}</span>
  </div>
{/if}
