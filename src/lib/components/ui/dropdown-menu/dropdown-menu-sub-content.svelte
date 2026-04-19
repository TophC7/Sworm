<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
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

<DropdownMenu.SubContent
  class={cn('z-50 min-w-[160px] rounded-lg border border-edge bg-raised py-1 text-base shadow-popover', className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
</DropdownMenu.SubContent>
