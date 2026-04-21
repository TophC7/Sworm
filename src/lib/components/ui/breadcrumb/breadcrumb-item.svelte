<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import type { HTMLLiAttributes } from 'svelte/elements'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    ...rest
  }: HTMLLiAttributes & {
    class?: string
    children?: Snippet
  } = $props()
</script>

<li class={cn('inline-flex min-w-0 items-center gap-1.5', className)} {...rest}>
  {#if children}{@render children()}{/if}
</li>
