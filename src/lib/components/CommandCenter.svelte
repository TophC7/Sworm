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
    CommandSeparator
  } from '$lib/components/ui/command'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import RebindDialog from '$lib/components/RebindDialog.svelte'
  import { HistoryIcon, PencilIcon } from '$lib/icons/lucideExports'
  import { cn } from '$lib/utils/cn'
  import {
    getAppCommandGroups,
    getEditorCommandGroups,
    type Command as CommandType,
    type CommandConfirm,
    type FileCallbacks
  } from '$lib/commands/index.svelte'
  import { getRecentCommandIds, recordRecentCommand } from '$lib/commands/recents.svelte'
  import { isCommandPaletteOpen, setCommandPaletteOpen } from '$lib/stores/ui.svelte'
  import { isTextEditorFocused } from '$lib/renderers/monaco/text/actions.svelte'
  import { getEffectiveSpec } from '$lib/stores/shortcutOverrides.svelte'
  import { splitShortcut } from '$lib/utils/keybindings.svelte'

  let { onNewProject, onSettings }: FileCallbacks = $props()

  // Rebind flow — when the user clicks the pencil icon next to a row's
  // shortcut, we close the palette and open this dialog. Keeping them
  // mutually exclusive avoids stacked modal focus weirdness and lets the
  // rebind dialog fully own keyboard capture.
  let rebindTarget = $state<{ id: string; label: string; defaultSpec: string | undefined } | null>(null)

  function openRebind(cmd: CommandType, event: Event) {
    event.preventDefault()
    event.stopPropagation()
    setCommandPaletteOpen(false)
    rebindTarget = { id: cmd.id, label: cmd.label, defaultSpec: cmd.shortcut }
  }

  function closeRebind() {
    rebindTarget = null
  }

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

  // Build a synthetic "Recent" group from the persisted id list.
  // Only surfaced in app mode, and only when search is empty — filtering
  // should hit the real groups so every match is discoverable without
  // duplicates across Recent + its source section.
  let recentGroup = $derived.by<import('$lib/commands/index.svelte').CommandGroup | null>(() => {
    if (isEditorMode) return null
    const ids = getRecentCommandIds()
    if (ids.length === 0) return null
    const byId = new Map<string, CommandType>()
    for (const g of appGroups) {
      for (const cmd of g.commands) byId.set(cmd.id, cmd)
    }
    const recents: CommandType[] = []
    for (const id of ids) {
      const cmd = byId.get(id)
      if (!cmd) continue
      recents.push({ ...cmd, id: `recent:${cmd.id}`, icon: cmd.icon ?? HistoryIcon })
    }
    if (recents.length === 0) return null
    return { heading: 'Recent', commands: recents }
  })

  let activeGroups = $derived.by(() => {
    const source = isEditorMode ? editorGroups : appGroups
    const withRecents = recentGroup && !filterQuery ? [recentGroup, ...source] : source
    if (!filterQuery) return withRecents

    return withRecents
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
  // re-running when isTextEditorFocused changes while already open.
  $effect(() => {
    if (open) {
      search = untrack(() => isTextEditorFocused()) ? '> ' : ''
    } else {
      search = ''
    }
  })

  function run(cmd: CommandType) {
    // Strip the `recent:` prefix we add when cloning into the Recent group
    // so both entry points record under the original command id.
    const baseId = cmd.id.startsWith('recent:') ? cmd.id.slice('recent:'.length) : cmd.id
    if (!isEditorMode) recordRecentCommand(baseId)
    setCommandPaletteOpen(false)
    search = ''
    // Tick delay so the dialog closes before the handler fires
    // (avoids focus conflicts with settings dialog, file picker, etc.)
    requestAnimationFrame(cmd.onSelect)
  }
</script>

<Dialog.Root {open} onOpenChange={(v) => setCommandPaletteOpen(v)}>
  <Dialog.Portal>
    <Dialog.Overlay class="fixed inset-0 z-50 bg-ground/70 backdrop-blur-sm" />
    <Dialog.Content
      class="fixed inset-0 z-50 flex translate-y-[-5vh] items-start justify-center pt-[20vh]"
      aria-label="Command center"
    >
      <div class={cn('w-full max-w-xl overflow-hidden rounded-xl border border-edge', 'bg-raised shadow-popover')}>
        <Command vimBindings={false} shouldFilter={false}>
          <CommandInput placeholder={isEditorMode ? 'Editor command...' : 'Type a command...'} bind:value={search} />
          <CommandList class="max-h-[50vh] [scroll-padding-block:0.5rem]">
            <CommandEmpty />

            {#each activeGroups as group, i (group.heading)}
              {#if i > 0}
                <CommandSeparator />
              {/if}
              <CommandGroup heading={group.heading}>
                {#each group.commands as cmd (cmd.id)}
                  {@const Icon = cmd.icon}
                  {@const effectiveShortcut = getEffectiveSpec(cmd.id, cmd.shortcut)}
                  <CommandItem value={cmd.id} keywords={cmd.keywords} onSelect={() => run(cmd)} class="group">
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
                    {#if effectiveShortcut || cmd.shortcut}
                      {@const parts = splitShortcut(effectiveShortcut)}
                      <span class="ml-auto flex items-center gap-1.5">
                        <button
                          type="button"
                          aria-label="Rebind shortcut"
                          title="Rebind shortcut"
                          onpointerdown={(e) => openRebind(cmd, e)}
                          class="rounded p-1 text-muted opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100 hover:bg-raised/70 hover:text-fg focus-visible:opacity-100"
                        >
                          <PencilIcon class="size-3" />
                        </button>
                        {#if parts.length > 0}
                          <KbdGroup>
                            {#each parts as part, i (i)}
                              {#if i > 0}<span class="text-subtle">+</span>{/if}
                              <Kbd>{part}</Kbd>
                            {/each}
                          </KbdGroup>
                        {:else}
                          <span class="text-xs text-subtle italic">unbound</span>
                        {/if}
                      </span>
                    {/if}
                  </CommandItem>
                {/each}
              </CommandGroup>
            {/each}
          </CommandList>
        </Command>
        <!-- Footer hints — surfaces core palette shortcuts so users learn -->
        <!-- navigation/editor-mode affordances without a separate help pane. -->
        <div
          class="flex items-center justify-between gap-4 border-t border-edge bg-surface/60 px-3 py-2 text-xs text-muted"
        >
          <div class="flex items-center gap-4">
            <span class="flex items-center gap-1.5">
              <Kbd>↑</Kbd>
              <Kbd>↓</Kbd>
              navigate
            </span>
            <span class="flex items-center gap-1.5">
              <Kbd>↵</Kbd>
              run
            </span>
            {#if !isEditorMode}
              <span class="flex items-center gap-1.5">
                <Kbd>&gt;</Kbd>
                editor mode
              </span>
            {/if}
          </div>
          <span class="flex items-center gap-1.5">
            <Kbd>Esc</Kbd>
            close
          </span>
        </div>
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

{#if rebindTarget}
  <RebindDialog
    open={true}
    commandId={rebindTarget.id}
    commandLabel={rebindTarget.label}
    defaultSpec={rebindTarget.defaultSpec}
    onClose={closeRebind}
  />
{/if}
