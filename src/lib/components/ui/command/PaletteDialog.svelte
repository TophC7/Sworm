<script lang="ts">
  import { Dialog as DialogPrimitive } from 'bits-ui'
  import type { Snippet } from 'svelte'
  import * as Dialog from '../dialog'

  let {
    open,
    onOpenChange,
    label,
    children,
    onkeydown,
    onOpenAutoFocus
  }: {
    open: boolean
    onOpenChange: (open: boolean) => void
    label: string
    children?: Snippet
    onkeydown?: DialogPrimitive.ContentProps['onkeydown']
    onOpenAutoFocus?: DialogPrimitive.ContentProps['onOpenAutoFocus']
  } = $props()
</script>

<DialogPrimitive.Root {open} {onOpenChange}>
  <DialogPrimitive.Portal>
    <Dialog.Overlay />
    <!-- Keep the modal boundary on the palette, not the viewport, so backdrop clicks dismiss it. -->
    <DialogPrimitive.Content
      data-slot="palette-dialog"
      class="fixed inset-x-3 top-[15vh] z-50 mx-auto flex max-h-[80dvh] max-w-3xl flex-col overflow-hidden rounded-xl border border-edge bg-raised shadow-popover"
      aria-label={label}
      {onkeydown}
      {onOpenAutoFocus}
    >
      {#if children}{@render children()}{/if}
    </DialogPrimitive.Content>
  </DialogPrimitive.Portal>
</DialogPrimitive.Root>
