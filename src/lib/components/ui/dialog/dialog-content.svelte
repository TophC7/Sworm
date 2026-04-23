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

  // Tie registration to actual DOM presence. An earlier version ran
  // registerModal from a component-level `$effect`, but that fires on
  // *Svelte* mount of this wrapper — which happens for every dialog
  // whose parent template sits in the tree, open or closed. The result
  // was ~9 phantom modals reporting "open" at rest, blocking every
  // focus-handoff that checks `isAnyModalOpen()`. `{@attach}` only
  // runs when the node is placed in the DOM, which mirrors bits-ui's
  // open-state presence exactly.
  function registerPresence() {
    return registerModal({
      isOpen: () => true,
      close: onModalClose
    })
  }
</script>

<Dialog.Portal>
  <DialogOverlay />
  <Dialog.Content class="fixed inset-0 z-50 flex items-center justify-center p-5" {...rest}>
    <div
      class={cn('w-full max-w-[480px] rounded-2xl border border-edge bg-raised p-5 shadow-popover', className)}
      {@attach registerPresence}
    >
      {#if children}{@render children()}{/if}
    </div>
  </Dialog.Content>
</Dialog.Portal>
