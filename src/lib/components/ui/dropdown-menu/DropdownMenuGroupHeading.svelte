<script lang="ts">
  import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuGroupHeadingClass } from '../menu-recipe'

  // Must sit inside DropdownMenu.Group or DropdownMenu.RadioGroup: bits reads the group context.
  let {
    ref = $bindable(null),
    class: className,
    inset,
    ...rest
  }: DropdownMenuPrimitive.GroupHeadingProps & {
    inset?: boolean
  } = $props()
</script>

<DropdownMenuPrimitive.GroupHeading
  bind:ref
  data-slot="dropdown-menu-group-heading"
  data-inset={inset || undefined}
  class={cn(menuGroupHeadingClass, className)}
  {...rest}
/>
