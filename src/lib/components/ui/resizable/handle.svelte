<script lang="ts">
  import { PaneResizer } from 'paneforge'
  import { cn } from '$lib/utils/cn'

  let {
    class: className,
    ...rest
  }: {
    class?: string
  } = $props()
</script>

<PaneResizer
  class={cn(
    'shrink-0 bg-edge/50 transition-colors hover:bg-accent/40 focus-visible:shadow-focus-ring focus-visible:outline-none',
    'data-[direction=horizontal]:w-1 data-[direction=horizontal]:cursor-col-resize',
    'data-[direction=vertical]:h-1 data-[direction=vertical]:cursor-row-resize',
    className
  )}
  {...rest}
/>
