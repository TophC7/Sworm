<script lang="ts">
  import { ChevronRight } from '$lib/icons/lucideExports'
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

<span aria-hidden="true" class={cn('inline-flex items-center text-subtle', className)} {...rest}>
  {#if children}
    {@render children()}
  {:else}
    <ChevronRight size={12} />
  {/if}
</span>
