<script lang="ts">
  import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'
  import { cn, type WithoutChild } from '$lib/utils/cn'
  import { ChevronRight } from '$lib/icons/lucideExports'
  import { menuItemClass } from '../menu-recipe'

  let {
    ref = $bindable(null),
    class: className,
    inset,
    children,
    ...rest
  }: WithoutChild<ContextMenuPrimitive.SubTriggerProps> & {
    inset?: boolean
  } = $props()
</script>

<ContextMenuPrimitive.SubTrigger
  bind:ref
  data-slot="context-menu-sub-trigger"
  data-inset={inset || undefined}
  class={cn(menuItemClass, 'data-[state=open]:bg-surface', className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
  <ChevronRight size={14} class="ms-auto" />
</ContextMenuPrimitive.SubTrigger>
