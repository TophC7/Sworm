<script lang="ts">
  import { ScrollArea as ScrollAreaPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'

  let {
    class: className,
    orientation = 'vertical',
    children,
    ...rest
  }: {
    class?: string
    orientation?: 'vertical' | 'horizontal' | 'both'
    children?: Snippet
  } = $props()
</script>

<ScrollAreaPrimitive.Root class={cn('relative overflow-hidden', className)} {...rest}>
  <ScrollAreaPrimitive.Viewport class="h-full w-full">
    {#if children}{@render children()}{/if}
  </ScrollAreaPrimitive.Viewport>
  {#if orientation !== 'horizontal'}
    <ScrollAreaPrimitive.Scrollbar
      orientation="vertical"
      class="flex w-1.5 touch-none p-px transition-opacity select-none hover:w-2"
    >
      <ScrollAreaPrimitive.Thumb class="relative flex-1 rounded-full bg-edge" />
    </ScrollAreaPrimitive.Scrollbar>
  {/if}
  {#if orientation !== 'vertical'}
    <ScrollAreaPrimitive.Scrollbar
      orientation="horizontal"
      class="flex h-1.5 touch-none flex-col p-px transition-opacity select-none hover:h-2"
    >
      <ScrollAreaPrimitive.Thumb class="relative flex-1 rounded-full bg-edge" />
    </ScrollAreaPrimitive.Scrollbar>
  {/if}
</ScrollAreaPrimitive.Root>
