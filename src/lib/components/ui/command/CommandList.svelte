<script lang="ts">
  import { Command } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    ref = $bindable(null),
    children,
    ...rest
  }: {
    class?: string
    ref?: HTMLElement | null
    children?: Snippet
  } = $props()
</script>

<Command.List bind:ref class={cn('max-h-[300px] overflow-x-hidden overflow-y-auto p-1', className)} {...rest}>
  {#if children}{@render children()}{/if}
</Command.List>
