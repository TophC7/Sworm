<script lang="ts">
  import { Dialog } from 'bits-ui'
  import {
    Command,
    CommandEmpty,
    CommandGroup,
    CommandInput,
    CommandItem,
    CommandList,
    CommandSeparator,
    CommandShortcut
  } from '$lib/components/ui/command'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import { cn } from '$lib/utils/cn'
  import { getCommandGroups, type CommandConfirm, type FileCallbacks } from '$lib/commands/index.svelte'
  import { isCommandPaletteOpen, setCommandPaletteOpen } from '$lib/stores/ui.svelte'

  let { onNewProject, onSettings }: FileCallbacks = $props()

  // Open state lives in ui.svelte.ts, toggled by shortcuts.svelte.ts
  let open = $derived(isCommandPaletteOpen())
  let search = $state('')

  let commandGroups = $derived(getCommandGroups({ onNewProject, onSettings }))

  // Collect unique confirm dialogs (deduplicated by reference)
  let confirms = $derived([
    ...new Set(
      commandGroups
        .flatMap((g) => g.commands)
        .map((c) => c.confirm)
        .filter((c): c is CommandConfirm => c !== undefined)
    )
  ])

  function run(handler: () => void) {
    setCommandPaletteOpen(false)
    search = ''
    // Tick delay so the dialog closes before the handler fires
    // (avoids focus conflicts with settings dialog, file picker, etc.)
    requestAnimationFrame(handler)
  }
</script>

<Dialog.Root
  {open}
  onOpenChange={(v) => {
    setCommandPaletteOpen(v)
    if (!v) search = ''
  }}
>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-50 bg-ground/70 backdrop-blur-sm" />
    <Dialog.Content class="fixed inset-0 z-50 flex items-start justify-center pt-[20vh]" aria-label="Command center">
      <div
        class={cn(
          'w-full max-w-[520px] overflow-hidden rounded-xl border border-edge',
          'bg-raised shadow-[0_16px_48px_rgba(0,0,0,0.5)]'
        )}
      >
        <Command loop vimBindings={false}>
          <CommandInput placeholder="Type a command..." bind:value={search} />
          <CommandList>
            <CommandEmpty />

            {#each commandGroups as group, i (group.heading)}
              {#if i > 0}
                <CommandSeparator />
              {/if}
              <CommandGroup heading={group.heading}>
                {#each group.commands as cmd (cmd.id)}
                  {@const Icon = cmd.icon}
                  <CommandItem value={cmd.id} keywords={cmd.keywords} onSelect={() => run(cmd.onSelect)}>
                    {#if Icon}
                      <Icon />
                    {:else if cmd.iconSrc}
                      <img src={cmd.iconSrc} alt="" class="h-4 w-4 shrink-0 opacity-60" />
                    {/if}
                    {#if cmd.subtitle}
                      <span class="truncate">{cmd.label}</span>
                      <span class="ml-auto truncate text-xs text-subtle">{cmd.subtitle}</span>
                    {:else}
                      {cmd.label}
                    {/if}
                    {#if cmd.shortcut}
                      <CommandShortcut>{cmd.shortcut}</CommandShortcut>
                    {/if}
                  </CommandItem>
                {/each}
              </CommandGroup>
            {/each}
          </CommandList>
        </Command>
      </div>
    </Dialog.Content>
  </Dialog.Portal>
</Dialog.Root>

{#each confirms as confirm (confirm.title)}
  <ConfirmDialog
    open={confirm.isOpen()}
    title={confirm.title}
    message={confirm.message}
    confirmLabel={confirm.confirmLabel}
    onConfirm={confirm.onConfirm}
    onCancel={confirm.onCancel}
  />
{/each}
