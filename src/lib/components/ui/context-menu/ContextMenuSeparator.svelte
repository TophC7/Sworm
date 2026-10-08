<script lang="ts">
  import { ContextMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { menuSeparatorClass } from '../menu-recipe'

  let { class: className, ...rest }: { class?: string } = $props()
</script>

<ContextMenu.Separator class={cn(menuSeparatorClass, className)} {...rest} />
