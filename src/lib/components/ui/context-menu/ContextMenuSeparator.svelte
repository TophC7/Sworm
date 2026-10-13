<script lang="ts">
  import { ContextMenu as ContextMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuSeparatorClass } from '../menu-recipe'

  let { ref = $bindable(null), class: className, ...rest }: ContextMenuPrimitive.SeparatorProps = $props()
</script>

<ContextMenuPrimitive.Separator
  bind:ref
  data-slot="context-menu-separator"
  class={cn(menuSeparatorClass, className)}
  {...rest}
/>
