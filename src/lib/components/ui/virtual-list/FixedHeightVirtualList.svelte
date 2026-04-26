<!--
  @component
  Fixed-height virtual list for long flat sequences.

  Renders only the rows currently inside (or near) the visible window
  and absolute-positions them within a spacer sized to the full content
  height. Only kicks in once `items.length` exceeds `threshold` and a
  scrollable ancestor was found at mount; below the threshold, rows
  render in normal flow so small lists pay no virtualization tax.

  Caller responsibilities:
   - Every row must be exactly `rowHeight` px tall. Variable-height
     rows are not supported here. DiffStack uses its own measurement-
     based pipeline; a `VariableHeightVirtualList` sibling should land
     before any variable-height consumers share this primitive. See
     TODO.md.
   - The nearest scrollable ancestor (overflow-y: auto/scroll) is the
     viewport. Wrap this component in a sized scroller.

  @param items Flat sequence to render. Mutating the array in place
    will not trigger reactivity; reassign or splice through `$state`.
  @param rowHeight Height in CSS pixels of every row. Must match the
    row markup's actual height or rows overlap or leave gaps.
  @param overscan Rows to render past the visible window on each side
    so a quick scroll does not reveal blank rows during reflow.
  @param threshold Below this `items.length`, virtualization is
    skipped and rows render in normal flow.
  @param row Snippet that renders one item: `(item, absoluteIndex)`.
  @param key Optional row-identity function for the `{#each}` key.
    Pass when item identity is stable across reorders so Svelte can
    reuse DOM nodes and per-row state instead of rekeying by index.
  @param class Tailwind / utility classes merged onto the wrapper.
-->

<script lang="ts" generics="T">
  import type { Snippet } from 'svelte'
  import { cn } from '$lib/utils/cn'
  import { findScrollParent } from '$lib/utils/dom'

  let {
    items,
    rowHeight = 22,
    overscan = 8,
    threshold = 100,
    row,
    key,
    class: className = ''
  }: {
    items: T[]
    rowHeight?: number
    overscan?: number
    threshold?: number
    row: Snippet<[T, number]>
    key?: (item: T, index: number) => string | number
    class?: string
  } = $props()

  let anchorEl = $state<HTMLElement | null>(null)
  let hasScrollParent = $state(false)
  let scrollTop = $state(0)
  let containerHeight = $state(0)
  // The list's offset inside the scroll parent's content. Constant
  // across scroll (it's a content-space offset, not viewport-space)
  // but changes if siblings above us resize. Cached to avoid running
  // `getBoundingClientRect` on every reactive tick.
  let anchorOffset = $state(0)

  $effect(() => {
    const anchor = anchorEl
    if (!anchor) return
    const parent = findScrollParent(anchor)
    if (!parent) {
      hasScrollParent = false
      return
    }
    hasScrollParent = true

    const recomputeOffset = () => {
      const a = anchor.getBoundingClientRect()
      const s = parent.getBoundingClientRect()
      anchorOffset = a.top - s.top + parent.scrollTop
    }
    const onScroll = () => {
      scrollTop = parent.scrollTop
    }
    const sync = () => {
      containerHeight = parent.clientHeight
      scrollTop = parent.scrollTop
      recomputeOffset()
    }
    sync()

    parent.addEventListener('scroll', onScroll, { passive: true })
    // Observe both the parent (viewport size) and the anchor (its
    // offset inside the parent's content shifts when siblings above
    // it grow/shrink, common in panels with collapsible headers).
    const ro = new ResizeObserver(sync)
    ro.observe(parent)
    ro.observe(anchor)

    return () => {
      parent.removeEventListener('scroll', onScroll)
      ro.disconnect()
      hasScrollParent = false
    }
  })

  let virtualize = $derived(items.length > threshold && hasScrollParent)
  let totalHeight = $derived(items.length * rowHeight)
  let windowStart = $derived.by(() => {
    if (!virtualize) return 0
    const localTop = scrollTop - anchorOffset
    return Math.max(0, Math.floor(localTop / rowHeight) - overscan)
  })
  let windowEnd = $derived.by(() => {
    if (!virtualize) return items.length
    const localTop = scrollTop - anchorOffset
    return Math.min(items.length, Math.ceil((localTop + containerHeight) / rowHeight) + overscan)
  })
  let offsetTop = $derived(windowStart * rowHeight)
</script>

<div bind:this={anchorEl} class={cn(className)}>
  {#if virtualize}
    <div class="relative" style:height="{totalHeight}px">
      <div class="absolute inset-x-0" style:top="{offsetTop}px">
        {#each items.slice(windowStart, windowEnd) as item, i (key ? key(item, windowStart + i) : windowStart + i)}
          {@render row(item, windowStart + i)}
        {/each}
      </div>
    </div>
  {:else}
    {#each items as item, i (key ? key(item, i) : i)}
      {@render row(item, i)}
    {/each}
  {/if}
</div>
