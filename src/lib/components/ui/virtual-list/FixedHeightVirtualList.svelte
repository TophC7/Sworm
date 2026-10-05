<!--
  @component
  Fixed-height virtual list for long flat sequences.

  Renders only the rows currently inside (or near) the visible window
  and absolute-positions them within a spacer sized to the full content
  height. Only kicks in once `items.length` exceeds `threshold` and a
  scrollable ancestor exists; below the threshold, rows render in normal
  flow. The viewport is measured live on scroll and on resize of the
  scroller or any ancestor between the list and the scroller.

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
  // Undefined until the mount effect resolves the scroll parent; null when none exists.
  let scrollParent = $state<HTMLElement | null | undefined>(undefined)
  // Viewport top relative to the list; negative while the list starts below it.
  let viewportTop = $state(0)
  let viewportHeight = $state(0)

  $effect(() => {
    const anchor = anchorEl
    if (!anchor) return
    const parent = findScrollParent(anchor)
    scrollParent = parent
    if (!parent) return

    // Live rects pick up sibling shifts that do not resize the list itself.
    const measure = () => {
      viewportTop = parent.getBoundingClientRect().top + parent.clientTop - anchor.getBoundingClientRect().top
      viewportHeight = parent.clientHeight
    }
    measure()
    parent.addEventListener('scroll', measure, { passive: true })
    const ro = new ResizeObserver(measure)
    ro.observe(parent)
    for (let el: HTMLElement | null = anchor; el && el !== parent; el = el.parentElement) ro.observe(el)

    return () => {
      parent.removeEventListener('scroll', measure)
      ro.disconnect()
    }
  })

  // Unresolved parents still render a bounded first window, never the full list.
  let virtualize = $derived(items.length > threshold && scrollParent !== null)
  let totalHeight = $derived(items.length * rowHeight)
  let windowStart = $derived(
    virtualize ? Math.min(items.length, Math.max(0, Math.floor(viewportTop / rowHeight) - overscan)) : 0
  )
  let windowEnd = $derived(
    virtualize
      ? Math.min(items.length, Math.max(windowStart, Math.ceil((viewportTop + viewportHeight) / rowHeight) + overscan))
      : items.length
  )
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
