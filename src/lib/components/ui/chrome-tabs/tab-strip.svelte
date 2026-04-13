<!--
  @component
  TabStrip -- shared keyboard-navigable app tablist chrome

  @param variant - visual variant for project or pane tab rows
  @param ariaLabel - accessible label for the tablist
  @param trailing - optional trailing snippet rendered after the tab buttons
-->

<script lang="ts" module>
  import { tv, type VariantProps } from 'tailwind-variants'

  export const tabStripVariants = tv({
    base: 'flex items-center overflow-x-auto scrollbar-none',
    variants: {
      variant: {
        project: 'min-w-0 flex-1 gap-px',
        pane: 'bg-ground border-b shrink-0 h-8 relative'
      }
    },
    defaultVariants: {
      variant: 'pane'
    }
  })

  export type TabStripVariant = VariantProps<typeof tabStripVariants>['variant']
</script>

<script lang="ts">
  import type { Snippet } from 'svelte'
  import { cn } from '$lib/utils/cn'

  let {
    variant = 'pane',
    ariaLabel = 'Tabs',
    class: className,
    children,
    trailing
  }: {
    variant?: TabStripVariant
    ariaLabel?: string
    class?: string
    children?: Snippet
    trailing?: Snippet
  } = $props()

  function handleKeydown(e: KeyboardEvent) {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return
    const tabs = Array.from((e.currentTarget as HTMLElement).querySelectorAll('[role="tab"]'))
    const current = tabs.indexOf(e.target as HTMLElement)
    if (current === -1) return
    e.preventDefault()
    const next = e.key === 'ArrowRight' ? (current + 1) % tabs.length : (current - 1 + tabs.length) % tabs.length
    ;(tabs[next] as HTMLElement).focus()
  }
</script>

<div
  class={cn(tabStripVariants({ variant }), className)}
  role="tablist"
  aria-label={ariaLabel}
  tabindex="-1"
  onkeydown={handleKeydown}
>
  {#if children}{@render children()}{/if}
  {#if trailing}{@render trailing()}{/if}
</div>
