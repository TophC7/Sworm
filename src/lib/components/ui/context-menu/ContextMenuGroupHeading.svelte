<script lang="ts">
  import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuGroupHeadingClass } from '../menu-recipe'

  // Must sit inside ContextMenu.Group or ContextMenu.RadioGroup: bits reads the group context.
  let {
    ref = $bindable(null),
    class: className,
    inset,
    ...rest
  }: ContextMenuPrimitive.GroupHeadingProps & {
    inset?: boolean
  } = $props()
</script>

<ContextMenuPrimitive.GroupHeading
  bind:ref
  data-slot="context-menu-group-heading"
  data-inset={inset || undefined}
  class={cn(menuGroupHeadingClass, className)}
  {...rest}
/>
