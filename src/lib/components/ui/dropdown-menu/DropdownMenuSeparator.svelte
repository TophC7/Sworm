<script lang="ts">
  import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuSeparatorClass } from '../menu-recipe'

  let { ref = $bindable(null), class: className, ...rest }: DropdownMenuPrimitive.SeparatorProps = $props()
</script>

<DropdownMenuPrimitive.Separator
  bind:ref
  data-slot="dropdown-menu-separator"
  class={cn(menuSeparatorClass, className)}
  {...rest}
/>
