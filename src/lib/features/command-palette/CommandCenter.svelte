<script lang="ts">
  import { untrack } from 'svelte'
  import { DialogRoot, DialogPortal, DialogOverlay, DialogContentRaw } from '$lib/components/ui/dialog'
  import {
    Command,
    CommandEmpty,
    CommandGroup,
    CommandInput,
    CommandItem,
    CommandList,
    CommandSeparator
  } from '$lib/components/ui/command'
  import { Kbd } from '$lib/components/ui/kbd'
  import RebindDialog from '$lib/features/command-palette/shortcuts/RebindDialog.svelte'
  import { HistoryIcon, PencilIcon } from '$lib/icons/lucideExports'
  import LucideIcon from '$lib/icons/LucideIcon.svelte'
  import { cn } from '$lib/utils/cn'
  import {
    getAppCommandGroups,
    getEditorCommandGroups,
    getTaskCommandGroups,
    type Command as CommandType
  } from '$lib/features/command-palette/commands/index.svelte'
  import { getRecentCommandIds, recordRecentCommand } from '$lib/features/command-palette/commands/recents.svelte'
  import {
    consumePendingInitialSearch,
    isCommandPaletteOpen,
    setCommandPaletteOpen
  } from '$lib/features/command-palette/state.svelte'
  import { isTextEditorFocused } from '$lib/features/editor/renderers/monaco/text/actions.svelte'
  import { getEffectiveBindings } from '$lib/features/command-palette/shortcuts/overrides.svelte'
  import { getShortcutCommand } from '$lib/features/command-palette/shortcuts/registry.svelte'
  import ShortcutPreview from '$lib/features/command-palette/shortcuts/ShortcutPreview.svelte'

  // Rebind flow. When the user clicks the pencil icon next to a row's
  // shortcut, we close the palette and open this dialog. Keeping them
  // mutually exclusive avoids stacked modal focus weirdness and lets the
  // rebind dialog fully own keyboard capture.
  let rebindTarget = $state<{ id: string; label: string; defaultKeybindings: string[] } | null>(null)

  function openRebind(cmd: CommandType, event: Event) {
    event.preventDefault()
    event.stopPropagation()
    setCommandPaletteOpen(false)
    rebindTarget = {
      id: commandBindingId(cmd),
      label: cmd.label,
      defaultKeybindings: commandDefaultKeybindings(cmd)
    }
  }

  function closeRebind() {
    rebindTarget = null
  }

  let open = $derived(isCommandPaletteOpen())
  let search = $state('')
  let commandValue = $state('')
  let commandListNode = $state<HTMLElement | null>(null)
  let suppressPointerSelection = $state(false)
  let lastPointerPosition = $state<{ x: number; y: number } | null>(null)

  // Prefix modes: `>` routes to editor commands, `!` routes to tasks.
  // Default (no prefix) shows app commands + Recent.
  let isEditorMode = $derived(search.startsWith('>'))
  let isTaskMode = $derived(search.startsWith('!'))
  let filterQuery = $derived(isEditorMode || isTaskMode ? search.slice(1).trim() : search.trim())

  let appGroups = $derived(getAppCommandGroups())
  let editorGroups = $derived(getEditorCommandGroups())
  let taskGroups = $derived(getTaskCommandGroups())
  let scheduledRun = 0
  const COMMAND_NAV_KEYS = new Set(['ArrowUp', 'ArrowDown', 'Home', 'End', 'PageUp', 'PageDown'])

  // Filter commands by the query (case-insensitive substring on label, keywords, id)
  function matchesQuery(query: string, label: string, keywords: string[], id: string): boolean {
    if (!query) return true
    const q = query.toLowerCase()
    if (label.toLowerCase().includes(q)) return true
    if (id.toLowerCase().includes(q)) return true
    return keywords.some((kw) => kw.toLowerCase().includes(q))
  }

  // Build a synthetic "Recent" group from the persisted id list.
  // Only surfaced in app mode, and only when search is empty. Filtering
  // should hit the real groups so every match is discoverable without
  // duplicates across Recent + its source section.
  let recentGroup = $derived.by<import('$lib/features/command-palette/commands/index.svelte').CommandGroup | null>(
    () => {
      if (isEditorMode || isTaskMode) return null
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
    }
  )

  let activeGroups = $derived.by(() => {
    const source = isEditorMode ? editorGroups : isTaskMode ? taskGroups : appGroups
    const withRecents = recentGroup && !filterQuery ? [recentGroup, ...source] : source
    if (!filterQuery) return withRecents

    return withRecents
      .map((g) => ({
        ...g,
        commands: g.commands.filter((cmd) => matchesQuery(filterQuery, cmd.label, cmd.keywords, cmd.id))
      }))
      .filter((g) => g.commands.length > 0)
  })

  // Set initial search when palette opens/closes. untrack prevents
  // re-running when isTextEditorFocused changes while already open.
  // A pending search (from openCommandPaletteWithSearch, e.g. the
  // "Show Tasks" command) wins over the editor-focus fallback.
  $effect(() => {
    if (open) {
      const queued = untrack(() => consumePendingInitialSearch())
      if (queued !== null) {
        search = queued
      } else {
        search = untrack(() => isTextEditorFocused()) ? '> ' : ''
      }
    } else {
      search = ''
    }
  })

  function run(cmd: CommandType) {
    // Strip the `recent:` prefix we add when cloning into the Recent group
    // so both entry points record under the original command id.
    const baseId = cmd.id.startsWith('recent:') ? cmd.id.slice('recent:'.length) : cmd.id
    // Task entries and editor-mode entries are excluded from Recent:
    // task lists already live behind `!`, and editor commands are
    // context-specific so a stale recent could fire in the wrong tab.
    if (!isEditorMode && !isTaskMode) recordRecentCommand(baseId)
    setCommandPaletteOpen(false)
    search = ''
    const runId = ++scheduledRun
    // Frame delay so the dialog closes before the handler fires.
    // Bail if another run superseded it or the palette reopened meanwhile.
    requestAnimationFrame(() => {
      if (runId !== scheduledRun || isCommandPaletteOpen()) return
      cmd.onSelect()
    })
  }

  function commandDefaultKeybindings(cmd: CommandType): string[] {
    if (cmd.defaultKeybindings) return cmd.defaultKeybindings
    const registered = getShortcutCommand(commandBindingId(cmd))
    if (registered) return registered.defaultKeybindings
    return cmd.shortcut ? [cmd.shortcut] : []
  }

  function commandCanEditShortcut(cmd: CommandType): boolean {
    if (cmd.defaultKeybindings !== undefined || cmd.shortcut !== undefined) return true
    return getShortcutCommand(commandBindingId(cmd)) !== null
  }

  function commandBindingId(cmd: CommandType): string {
    return cmd.id.startsWith('recent:') ? cmd.id.slice('recent:'.length) : cmd.id
  }

  function commandItems(): HTMLElement[] {
    return Array.from(
      commandListNode?.querySelectorAll<HTMLElement>('[role="option"]:not([aria-disabled="true"])') ?? []
    )
  }

  function selectedCommandIndex(items: HTMLElement[]): number {
    return items.findIndex(
      (item) => item.getAttribute('data-value') === commandValue || item.hasAttribute('data-selected')
    )
  }

  function stepCommandSelection(direction: 1 | -1): void {
    const items = commandItems()
    if (items.length === 0) return

    const currentIndex = selectedCommandIndex(items)
    const fallbackIndex = direction > 0 ? -1 : 1
    selectCommandAt((currentIndex >= 0 ? currentIndex : fallbackIndex) + direction)
  }

  function selectCommandAt(index: number): void {
    const items = commandItems()
    if (items.length === 0) return

    const item = items[Math.max(0, Math.min(index, items.length - 1))]
    const value = item.getAttribute('data-value')
    if (!value) return

    commandValue = value
    item.scrollIntoView({ block: 'nearest' })
  }

  function pageCommandSelection(direction: 1 | -1): void {
    const items = commandItems()
    if (items.length === 0) return

    const currentIndex = selectedCommandIndex(items)
    const anchorIndex = currentIndex >= 0 ? currentIndex : direction > 0 ? 0 : items.length - 1
    const anchor = items[anchorIndex] ?? items[0]
    const rowHeight = Math.max(1, anchor.getBoundingClientRect().height)
    const visibleRows = Math.max(1, Math.floor((commandListNode?.clientHeight ?? rowHeight) / rowHeight) - 1)
    selectCommandAt(anchorIndex + direction * visibleRows)
  }

  function handleCommandKeydown(event: KeyboardEvent): void {
    if (event.defaultPrevented || event.altKey || event.ctrlKey || event.metaKey || !COMMAND_NAV_KEYS.has(event.key)) {
      return
    }

    // Keyboard scrolling can fire pointermove under a stationary cursor.
    // Keep hover selection disabled until the pointer actually moves.
    suppressPointerSelection = true
    event.preventDefault()
    event.stopPropagation()

    if (event.key === 'Home') {
      selectCommandAt(0)
    } else if (event.key === 'End') {
      selectCommandAt(commandItems().length - 1)
    } else if (event.key === 'PageUp') {
      pageCommandSelection(-1)
    } else if (event.key === 'PageDown') {
      pageCommandSelection(1)
    } else if (event.key === 'ArrowUp') {
      stepCommandSelection(-1)
    } else if (event.key === 'ArrowDown') {
      stepCommandSelection(1)
    }
  }

  function handleCommandPointerMove(event: PointerEvent): void {
    const position = { x: event.clientX, y: event.clientY }
    const last = lastPointerPosition
    lastPointerPosition = position
    if (!last) {
      if (event.movementX !== 0 || event.movementY !== 0) suppressPointerSelection = false
      return
    }
    if (last.x !== position.x || last.y !== position.y) suppressPointerSelection = false
  }

  function handleCommandPointerLeave(): void {
    lastPointerPosition = null
    suppressPointerSelection = false
  }
</script>

<DialogRoot {open} onOpenChange={(v) => setCommandPaletteOpen(v)}>
  <DialogPortal>
    <DialogOverlay />
    <DialogContentRaw
      class="fixed inset-0 z-50 flex translate-y-[-5vh] items-start justify-center pt-[20vh]"
      aria-label="Command center"
    >
      <div class={cn('w-full max-w-xl overflow-hidden rounded-xl border border-edge', 'bg-raised shadow-popover')}>
        <Command
          bind:value={commandValue}
          vimBindings={false}
          shouldFilter={false}
          disablePointerSelection={suppressPointerSelection}
          onkeydown={handleCommandKeydown}
          onpointermovecapture={handleCommandPointerMove}
          onpointerleavecapture={handleCommandPointerLeave}
          class="overflow-visible rounded-none"
        >
          <CommandInput
            placeholder={isEditorMode ? 'Editor command...' : isTaskMode ? 'Run task...' : 'Type a command...'}
            bind:value={search}
          />
          <CommandList bind:ref={commandListNode} class="max-h-[50vh] [scroll-padding-block:0.5rem]">
            <CommandEmpty />

            {#each activeGroups as group, i (group.heading)}
              {#if i > 0}
                <CommandSeparator />
              {/if}
              <CommandGroup heading={group.heading}>
                {#each group.commands as cmd (cmd.id)}
                  {@const Icon = cmd.icon}
                  {@const defaultKeybindings = commandDefaultKeybindings(cmd)}
                  {@const effectiveShortcuts = getEffectiveBindings(commandBindingId(cmd), defaultKeybindings)}
                  {@const canEditShortcut = commandCanEditShortcut(cmd)}
                  <CommandItem value={cmd.id} keywords={cmd.keywords} onSelect={() => run(cmd)} class="group">
                    {#if Icon}
                      <Icon />
                    {:else if cmd.iconSrc}
                      <img src={cmd.iconSrc} alt="" class="h-4 w-4 shrink-0 opacity-60" />
                    {:else if cmd.lucideIcon}
                      <!-- LucideIcon lazy-loads a Svelte chunk per icon name.
                           Callers pass a fallback (e.g. 'terminal') at command
                           build time so invalid names degrade to no icon rather
                           than layout shift. -->
                      <LucideIcon name={cmd.lucideIcon} size={16} class="shrink-0 opacity-60" />
                    {/if}
                    {#if cmd.subtitle}
                      <span class="truncate">{cmd.label}</span>
                      <span class="ml-auto truncate text-xs text-subtle">{cmd.subtitle}</span>
                    {:else}
                      {cmd.label}
                    {/if}
                    {#if canEditShortcut || effectiveShortcuts.length > 0}
                      {@const primaryShortcut = effectiveShortcuts[0]}
                      <span class="ml-auto flex items-center gap-1.5">
                        {#if canEditShortcut}
                          <button
                            type="button"
                            aria-label={primaryShortcut ? 'Edit shortcut' : 'Add shortcut'}
                            title={primaryShortcut ? 'Edit shortcut' : 'Add shortcut'}
                            onpointerdown={(e) => openRebind(cmd, e)}
                            class={cn(
                              'invisible cursor-pointer rounded text-muted group-focus-within:visible group-hover:visible group-data-selected:visible hover:bg-raised/70 hover:text-fg focus-visible:visible focus-visible:shadow-focus-ring focus-visible:outline-none',
                              primaryShortcut
                                ? 'p-1'
                                : 'flex w-28 items-center justify-end gap-1 px-1.5 py-1 text-xs whitespace-nowrap'
                            )}
                          >
                            <PencilIcon class="size-3" />
                            {#if !primaryShortcut}
                              <span>Add shortcut</span>
                            {/if}
                          </button>
                        {/if}
                        {#if primaryShortcut}
                          <ShortcutPreview spec={primaryShortcut} class="flex-nowrap" />
                          {#if effectiveShortcuts.length > 1}
                            <span class="text-xs text-subtle">+{effectiveShortcuts.length - 1}</span>
                          {/if}
                        {/if}
                      </span>
                    {/if}
                  </CommandItem>
                {/each}
              </CommandGroup>
            {/each}
          </CommandList>
        </Command>
        <!-- Footer hints surface core palette shortcuts so users learn -->
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
            {#if !isEditorMode && !isTaskMode}
              <span class="flex items-center gap-1.5">
                <Kbd>&gt;</Kbd>
                editor
              </span>
              <span class="flex items-center gap-1.5">
                <Kbd>!</Kbd>
                tasks
              </span>
            {/if}
          </div>
          <span class="flex items-center gap-1.5">
            <Kbd>Esc</Kbd>
            close
          </span>
        </div>
      </div>
    </DialogContentRaw>
  </DialogPortal>
</DialogRoot>

{#if rebindTarget}
  <RebindDialog
    open={true}
    commandId={rebindTarget.id}
    commandLabel={rebindTarget.label}
    defaultKeybindings={rebindTarget.defaultKeybindings}
    onClose={closeRebind}
  />
{/if}
