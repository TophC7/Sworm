<!-- Keyboard-navigable global title-bar tab list. -->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import { cn } from '$lib/utils/cn'

  let {
    ariaLabel = 'Tabs',
    class: className,
    children,
    trailing
  }: {
    ariaLabel?: string
    class?: string
    children?: Snippet
    trailing?: Snippet
  } = $props()

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight') return
    const tabs = Array.from((event.currentTarget as HTMLElement).querySelectorAll('[role="tab"]'))
    const current = tabs.indexOf(event.target as HTMLElement)
    if (current === -1) return
    event.preventDefault()
    const next = event.key === 'ArrowRight' ? (current + 1) % tabs.length : (current - 1 + tabs.length) % tabs.length
    ;(tabs[next] as HTMLElement).focus()
  }
</script>

<div
  class={cn('relative flex h-full min-w-0 flex-1 scrollbar-none items-center gap-px overflow-x-auto', className)}
  role="tablist"
  aria-label={ariaLabel}
  tabindex="-1"
  onkeydown={handleKeydown}
>
  {#if children}{@render children()}{/if}
  {#if trailing}{@render trailing()}{/if}
</div>
