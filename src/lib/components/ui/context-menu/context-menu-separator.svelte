<script lang="ts">
  import { ContextMenu } from 'bits-ui'
  import { cn } from '$lib/utils/cn'

  let { class: className, ...rest }: { class?: string } = $props()
</script>

<ContextMenu.Separator class={cn('mx-2 my-1 h-px bg-edge', className)} {...rest} />
