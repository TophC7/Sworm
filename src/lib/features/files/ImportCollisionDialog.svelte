<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import * as Dialog from '$lib/components/ui/dialog'
  import { Input } from '$lib/components/ui/input'

  let {
    open = false,
    sourceName,
    destinationPath,
    renameValue = '',
    onRenameValueChange,
    onReplace,
    onSkip,
    onRename,
    onCancel
  }: {
    open?: boolean
    sourceName: string
    destinationPath: string
    renameValue?: string
    onRenameValueChange?: (value: string) => void
    onReplace?: () => void
    onSkip?: () => void
    onRename?: (value: string) => void
    onCancel?: () => void
  } = $props()

  const renameInputId = 'import-collision-rename'
</script>

<Dialog.Root
  {open}
  onOpenChange={(value) => {
    if (!value) onCancel?.()
  }}
>
  <Dialog.Content class="max-w-md">
    <Dialog.Title>Item Already Exists</Dialog.Title>
    <Dialog.Description>
      <span class="font-medium text-fg">{sourceName}</span> conflicts with
      <span class="font-medium text-fg">{destinationPath}</span>.
    </Dialog.Description>

    <div class="mt-3 space-y-1.5">
      <label for={renameInputId} class="text-sm font-medium text-muted uppercase">Rename To</label>
      <Input
        id={renameInputId}
        value={renameValue}
        placeholder={sourceName}
        oninput={(event: Event) => onRenameValueChange?.((event.currentTarget as HTMLInputElement).value)}
        onkeydown={(event: KeyboardEvent) => {
          if (event.key === 'Enter') onRename?.(renameValue)
        }}
      />
    </div>

    <Dialog.Footer class="mt-4 flex-wrap gap-2">
      <Button size="sm" variant="ghost" onclick={onSkip}>Skip</Button>
      <Button size="sm" variant="destructive" onclick={onReplace}>Replace</Button>
      <Button size="sm" variant="accent" onclick={() => onRename?.(renameValue)}>Rename</Button>
    </Dialog.Footer>
  </Dialog.Content>
</Dialog.Root>
