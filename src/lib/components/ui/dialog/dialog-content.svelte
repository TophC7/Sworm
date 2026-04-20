<script lang="ts">
  import { Dialog } from 'bits-ui'
  import { cn } from '$lib/utils/cn'
  import { registerModal } from '$lib/utils/modalRegistry.svelte'
  import type { Snippet } from 'svelte'
  import DialogOverlay from './dialog-overlay.svelte'

  let {
    class: className,
    children,
    onModalClose,
    ...rest
  }: {
    class?: string
    children?: Snippet
    /**
     * Optional. When provided, the global keybinding dispatcher will
     * call this to auto-dismiss the dialog before firing an action
     * shortcut. Omit for blocking dialogs (confirm prompts, required
     * decisions) — those stay up until the user answers them, but
     * still participate in focus-restore.
     */
    onModalClose?: () => void
  } = $props()

  // Auto-register with the modal registry. bits-ui only mounts
  // Dialog.Content while the dialog is actually open, so the mount
  // lifecycle mirrors the open state — no prop tracking needed.
  // Every dialog using this primitive participates in focus-restore
  // without any per-dialog boilerplate; passing `onModalClose` also
  // makes it transient-dismissable.
  $effect(() => {
    return registerModal({
      isOpen: () => true,
      close: onModalClose
    })
  })
</script>

<Dialog.Portal>
  <DialogOverlay />
  <Dialog.Content class="fixed inset-0 z-50 flex items-center justify-center p-5" {...rest}>
    <div class={cn('w-full max-w-[480px] rounded-2xl border border-edge bg-raised p-5 shadow-popover', className)}>
      {#if children}{@render children()}{/if}
    </div>
  </Dialog.Content>
</Dialog.Portal>
