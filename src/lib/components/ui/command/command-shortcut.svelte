<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    ...rest
  }: {
    class?: string
    children?: Snippet
  } = $props()
</script>

<kbd aria-hidden="true" class={cn('ml-auto text-xs tracking-widest text-subtle', className)} {...rest}>
  {#if children}{@render children()}{/if}
</kbd>
