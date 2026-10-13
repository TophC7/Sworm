<script lang="ts">
  import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuContentVariants } from '../menu-recipe'

  let {
    ref = $bindable(null),
    sideOffset = 4,
    class: className,
    ...rest
  }: DropdownMenuPrimitive.ContentProps = $props()
</script>

<DropdownMenuPrimitive.Portal>
  <DropdownMenuPrimitive.Content
    bind:ref
    data-slot="dropdown-menu-content"
    {sideOffset}
    class={cn(menuContentVariants({ source: 'dropdown' }), className)}
    {...rest}
  />
</DropdownMenuPrimitive.Portal>
