<script lang="ts">
  import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuContentVariants } from '../menu-recipe'

  let { ref = $bindable(null), class: className, ...rest }: DropdownMenuPrimitive.SubContentProps = $props()
</script>

<DropdownMenuPrimitive.SubContent
  bind:ref
  data-slot="dropdown-menu-sub-content"
  class={cn(menuContentVariants({ source: 'sub' }), className)}
  {...rest}
/>
