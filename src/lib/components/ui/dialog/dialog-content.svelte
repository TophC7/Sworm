<script lang="ts">
  import { Dialog } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import type { Snippet } from 'svelte'
  import DialogOverlay from './dialog-overlay.svelte'

  let {
    class: className,
    children,
    ...rest
  }: {
    class?: string
    children?: Snippet
  } = $props()
</script>

<Dialog.Portal>
  <DialogOverlay />
  <Dialog.Content class="fixed inset-0 z-50 flex items-center justify-center p-5" {...rest}>
    <div class={cn('w-full max-w-[480px] rounded-2xl border border-edge bg-raised p-5 shadow-popover', className)}>
      {#if children}{@render children()}{/if}
    </div>
  </Dialog.Content>
</Dialog.Portal>
