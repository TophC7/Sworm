<script lang="ts">
  import { DialogRoot, DialogContent, DialogTitle, DialogFooter } from '$lib/components/ui/dialog'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'

  let {
    open = false,
    title,
    value = $bindable(''),
    placeholder = '',
    confirmLabel = 'Confirm',
    cancelLabel = 'Cancel',
    onConfirm,
    onCancel
  }: {
    open?: boolean
    title: string
    value?: string
    placeholder?: string
    confirmLabel?: string
    cancelLabel?: string
    onConfirm?: () => void
    onCancel?: () => void
  } = $props()

  function handleKeydown(e: KeyboardEvent) {
    if (e.key === 'Enter') {
      e.preventDefault()
      onConfirm?.()
    }
  }
</script>

<DialogRoot
  bind:open
  onOpenChange={(v) => {
    if (!v) onCancel?.()
  }}
>
  <DialogContent>
    <DialogTitle>{title}</DialogTitle>
    <!-- svelte-ignore a11y_autofocus — focusing the sole input on mount is expected UX for a modal prompt -->
    <Input class="mt-2" bind:value {placeholder} onkeydown={handleKeydown} autofocus />
    <DialogFooter>
      <Button variant="outline" onclick={onCancel}>{cancelLabel}</Button>
      <Button variant="accent" onclick={onConfirm}>{confirmLabel}</Button>
    </DialogFooter>
  </DialogContent>
</DialogRoot>
