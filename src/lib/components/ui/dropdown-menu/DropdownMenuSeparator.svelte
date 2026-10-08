<script lang="ts">
  import { DropdownMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuSeparatorClass } from '../menu-recipe'

  let { class: className, ...rest }: { class?: string } = $props()
</script>

<DropdownMenu.Separator class={cn(menuSeparatorClass, className)} {...rest} />
