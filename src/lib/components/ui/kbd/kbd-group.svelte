<!--
  @component
  KbdGroup — horizontal container for a sequence of `<Kbd>` chips that
  represent a single combo. Inserts a faint "+" between each pair.
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
  data-slot="kbd-group"
  class={cn('inline-flex items-center gap-1 font-mono text-subtle select-none', className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
</kbd>
