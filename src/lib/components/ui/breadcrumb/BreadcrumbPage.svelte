<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    ...rest
  }: HTMLAttributes<HTMLSpanElement> & {
    class?: string
    children?: Snippet
  } = $props()
</script>

<span aria-current="page" class={cn('truncate font-semibold text-bright', className)} {...rest}>
  {#if children}{@render children()}{/if}
</span>
