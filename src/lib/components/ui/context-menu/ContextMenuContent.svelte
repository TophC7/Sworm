<script lang="ts">
  import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuContentVariants } from '../menu-recipe'

  let {
    ref = $bindable(null),
    class: className,
    ...rest
  }: ContextMenuPrimitive.ContentProps = $props()
</script>

<ContextMenuPrimitive.Portal>
  <ContextMenuPrimitive.Content
    bind:ref
    data-slot="context-menu-content"
    class={cn(menuContentVariants({ source: 'context' }), className)}
    {...rest}
  />
</ContextMenuPrimitive.Portal>
