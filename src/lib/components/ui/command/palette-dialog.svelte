<script lang="ts">
  import type { Dialog } from 'bits-ui'
  import type { Snippet } from 'svelte'
  import { DialogRoot, DialogPortal, DialogOverlay, DialogContentRaw } from '$lib/components/ui/dialog'

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
    onkeydown?: Dialog.ContentProps['onkeydown']
    onOpenAutoFocus?: Dialog.ContentProps['onOpenAutoFocus']
  } = $props()
</script>

<DialogRoot {open} {onOpenChange}>
  <DialogPortal>
    <DialogOverlay />
    <!-- Keep the modal boundary on the palette, not the viewport, so backdrop clicks dismiss it. -->
    <DialogContentRaw
      data-slot="palette-dialog"
      class="fixed inset-x-3 top-[15vh] z-50 mx-auto flex max-h-[80dvh] max-w-3xl flex-col overflow-hidden rounded-xl border border-edge bg-raised shadow-popover"
      aria-label={label}
      {onkeydown}
      {onOpenAutoFocus}
    >
      {#if children}{@render children()}{/if}
    </DialogContentRaw>
  </DialogPortal>
</DialogRoot>
