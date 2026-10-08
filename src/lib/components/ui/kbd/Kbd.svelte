<!--
  @component
  Kbd — individual keyboard key chip.

  Use inline for a single key ("Esc") or inside `<KbdGroup>` to visually
  join a combo like "Ctrl + Shift + T". Styling mirrors Sworm's warm
  base16 tokens rather than raw colors.
-->

<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'
  import type { HTMLAttributes } from 'svelte/elements'

  let {
    class: className,
    children,
    ...rest
  }: HTMLAttributes<HTMLElement> & { class?: string; children?: Snippet } = $props()
</script>

<kbd
  data-slot="kbd"
  class={cn(
    'pointer-events-none inline-flex h-6 min-w-6 items-center justify-center rounded-md border border-edge',
    'bg-surface px-1.5 font-mono text-sm font-medium text-fg select-none',
    'shadow-kbd-inset',
    className
  )}
  {...rest}
>
  {#if children}{@render children()}{/if}
</kbd>
