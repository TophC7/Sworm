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

<!-- The visible line stays thin; before: extends the grab area to ~12px -->
<PaneResizer
  class={cn(
    'relative z-10 shrink-0 bg-edge/50 transition-colors before:absolute before:inset-0 hover:bg-accent/40 focus-visible:shadow-focus-ring focus-visible:outline-none',
    'data-[direction=horizontal]:w-1 data-[direction=horizontal]:cursor-col-resize data-[direction=horizontal]:before:-inset-x-1',
    'data-[direction=vertical]:h-1 data-[direction=vertical]:cursor-row-resize data-[direction=vertical]:before:-inset-y-1',
    className
  )}
  {...rest}
/>
