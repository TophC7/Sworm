<script lang="ts">
  import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuContentVariants } from '../menu-recipe'

  let { ref = $bindable(null), class: className, ...rest }: ContextMenuPrimitive.SubContentProps = $props()
</script>

<ContextMenuPrimitive.SubContent
  bind:ref
  data-slot="context-menu-sub-content"
  class={cn(menuContentVariants({ source: 'sub' }), className)}
  {...rest}
/>
