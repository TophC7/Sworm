<script lang="ts">
  import { DropdownMenu as DropdownMenuPrimitive } from 'bits-ui'

  let { ref = $bindable(null), ...rest }: DropdownMenuPrimitive.GroupProps = $props()
</script>

<DropdownMenuPrimitive.Group bind:ref data-slot="dropdown-menu-group" {...rest} />
