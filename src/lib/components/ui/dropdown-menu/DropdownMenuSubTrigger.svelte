<script lang="ts">
  import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'
  import { cn, type WithoutChild } from '$lib/utils/cn'
  import { ChevronRight } from '$lib/icons/lucideExports'
  import { menuItemClass } from '../menu-recipe'

  let {
    ref = $bindable(null),
    class: className,
    inset,
    children,
    ...rest
  }: WithoutChild<DropdownMenuPrimitive.SubTriggerProps> & {
    inset?: boolean
  } = $props()
</script>

<DropdownMenuPrimitive.SubTrigger
  bind:ref
  data-slot="dropdown-menu-sub-trigger"
  data-inset={inset || undefined}
  class={cn(menuItemClass, 'data-[state=open]:bg-surface', className)}
  {...rest}
>
  {#if children}{@render children()}{/if}
  <ChevronRight size={14} class="ms-auto" />
</DropdownMenuPrimitive.SubTrigger>
