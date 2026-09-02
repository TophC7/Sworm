<script lang="ts">
  import { Popover } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    children,
    sideOffset = 8,
    align,
    ...rest
  }: {
    class?: string
    children?: Snippet
    sideOffset?: number
    align?: 'start' | 'center' | 'end'
  } = $props()
</script>

<Popover.Portal>
  <Popover.Content
    {sideOffset}
    {align}
    class={cn('z-50 rounded-lg border border-edge bg-raised text-base shadow-popover', className)}
    {...rest}
  >
    {#if children}{@render children()}{/if}
  </Popover.Content>
</Popover.Portal>
