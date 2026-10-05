<script lang="ts">
  import { ChevronRight } from '$lib/icons/lucideExports'
  import { cn } from '$lib/utils/cn'
  import type { HTMLLiAttributes } from 'svelte/elements'

  let {
    class: className,
    ...rest
  }: HTMLLiAttributes & {
    class?: string
  } = $props()
</script>

<li role="presentation" aria-hidden="true" class={cn('inline-flex items-center text-subtle', className)} {...rest}>
  <ChevronRight size={12} />
</li>
