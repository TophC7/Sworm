<script lang="ts">
  import { Command } from 'bits-ui'
  import { cn } from '$lib/utils/cn'

  let {
    class: className,
    ...rest
  }: {
    class?: string
  } = $props()
</script>

<Command.Separator class={cn('-mx-1 h-px bg-edge', className)} {...rest} />
