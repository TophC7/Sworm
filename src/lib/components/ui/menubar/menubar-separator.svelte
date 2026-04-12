<script lang="ts">
  import { cn } from '$lib/utils/cn'
  import { Menubar } from 'bits-ui'

  let { class: className, ...rest }: { class?: string } = $props()
</script>

<Menubar.Separator class={cn('mx-2 my-1 h-px bg-edge', className)} {...rest} />
