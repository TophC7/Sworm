<script lang="ts">
  import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuItemClass } from '../menu-recipe'

  let {
    ref = $bindable(null),
    class: className,
    inset,
    destructive = false,
    ...rest
  }: Omit<DropdownMenuPrimitive.ItemProps, 'onclick'> & {
    inset?: boolean
    destructive?: boolean
  } = $props()

  // Use onSelect: Bits checks disabled before the action, then closes the menu
  // even when that action immediately makes the row busy or opens a dialog.
</script>

<DropdownMenuPrimitive.Item
  bind:ref
  data-slot="dropdown-menu-item"
  data-inset={inset || undefined}
  data-destructive={destructive || undefined}
  class={cn(menuItemClass, className)}
  {...rest}
/>
