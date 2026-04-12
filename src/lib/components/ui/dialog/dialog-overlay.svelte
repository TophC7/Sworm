<script lang="ts">
  import { Dialog } from 'bits-ui'
  import { cn } from '$lib/utils/cn'

  let { class: className, ...rest }: { class?: string } = $props()
</script>

<Dialog.Overlay class={cn('fixed inset-0 z-50 bg-ground/70 backdrop-blur-sm', className)} {...rest} />
