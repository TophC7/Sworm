<script lang="ts">
  import { untrack } from 'svelte'
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
  import {
    getAppCommandGroups,
    getEditorCommandGroups,
    type CommandConfirm,
    type FileCallbacks
  } from '$lib/commands/index.svelte'
  import { isCommandPaletteOpen, setCommandPaletteOpen } from '$lib/stores/ui.svelte'
  import { isEditorFocused } from '$lib/editor/editorActions.svelte'

  let { onNewProject, onSettings }: FileCallbacks = $props()

  let open = $derived(isCommandPaletteOpen())
  let search = $state('')

  // Detect `>` prefix — switches to editor command mode
  let isEditorMode = $derived(search.startsWith('>'))
  let filterQuery = $derived(isEditorMode ? search.slice(1).trim() : search.trim())

  let appGroups = $derived(getAppCommandGroups({ onNewProject, onSettings }))
  let editorGroups = $derived(getEditorCommandGroups())

  // Filter commands by the query (case-insensitive substring on label, keywords, id)
  function matchesQuery(query: string, label: string, keywords: string[], id: string): boolean {
    if (!query) return true
    const q = query.toLowerCase()
    if (label.toLowerCase().includes(q)) return true
    if (id.toLowerCase().includes(q)) return true
    return keywords.some((kw) => kw.toLowerCase().includes(q))
  }

  let activeGroups = $derived.by(() => {
    const source = isEditorMode ? editorGroups : appGroups
    if (!filterQuery) return source

    return source
      .map((g) => ({
        ...g,
        commands: g.commands.filter((cmd) => matchesQuery(filterQuery, cmd.label, cmd.keywords, cmd.id))
      }))
      .filter((g) => g.commands.length > 0)
  })

  // Collect unique confirm dialogs from app groups only (editor commands don't use them)
  let confirms = $derived([
    ...new Set(
      appGroups
        .flatMap((g) => g.commands)
        .map((c) => c.confirm)
        .filter((c): c is CommandConfirm => c !== undefined)
    )
  ])

  // Set initial search when palette opens/closes. untrack prevents
  // re-running when isEditorFocused changes while already open.
  $effect(() => {
    if (open) {
      search = untrack(() => isEditorFocused()) ? '> ' : ''
    } else {
      search = ''
    }
  })

  function run(handler: () => void) {
    setCommandPaletteOpen(false)
    search = ''
    // Tick delay so the dialog closes before the handler fires
    // (avoids focus conflicts with settings dialog, file picker, etc.)
    requestAnimationFrame(handler)
  }
</script>

<Dialog.Root {open} onOpenChange={(v) => setCommandPaletteOpen(v)}>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-50 bg-ground/70 backdrop-blur-sm" />
    <Dialog.Content class="fixed inset-0 z-50 flex items-start justify-center pt-[20vh]" aria-label="Command center">
      <div
        class={cn(
          'w-full max-w-[520px] overflow-hidden rounded-xl border border-edge',
          'bg-raised shadow-[0_16px_48px_rgba(0,0,0,0.5)]'
        )}
      >
        <Command loop vimBindings={false} shouldFilter={false}>
          <CommandInput placeholder={isEditorMode ? 'Editor command...' : 'Type a command...'} bind:value={search} />
          <CommandList>
            <CommandEmpty />

            {#each activeGroups as group, i (group.heading)}
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
