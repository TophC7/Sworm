<!--
  @component
  KeyboardShortcutsView: command keybinding editor for stable app and editor commands.
-->

<script lang="ts">
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { TabsList, TabsRoot, TabsTrigger } from '$lib/components/ui/tabs'
  import RebindDialog from '$lib/features/command-palette/shortcuts/RebindDialog.svelte'
  import ShortcutPreview from '$lib/features/command-palette/shortcuts/ShortcutPreview.svelte'
  import { clearShortcutOverride, getUserKeybindings } from '$lib/features/command-palette/shortcuts/overrides.svelte'
  import {
    findShortcutConflict,
    getShortcutInfos,
    type ShortcutCommandInfo
  } from '$lib/features/command-palette/shortcuts/registry.svelte'

  type Filter = 'all' | 'modified' | 'unassigned' | 'conflicts'
  const FILTERS = ['all', 'modified', 'unassigned', 'conflicts'] as const satisfies readonly Filter[]
  const FILTER_LABELS: Record<Filter, string> = {
    all: 'All',
    modified: 'Modified',
    unassigned: 'Unassigned',
    conflicts: 'Conflicts'
  }

  let search = $state('')
  let filter = $state<Filter>('all')
  let rebindTarget = $state<ShortcutCommandInfo | null>(null)

  function matchesSearch(command: ShortcutCommandInfo): boolean {
    const query = search.trim().toLowerCase()
    if (!query) return true
    return (
      command.label.toLowerCase().includes(query) ||
      command.id.toLowerCase().includes(query) ||
      command.group.toLowerCase().includes(query) ||
      command.keywords.some((keyword) => keyword.toLowerCase().includes(query))
    )
  }

  function isModified(command: ShortcutCommandInfo): boolean {
    return getUserKeybindings(command.id) !== null
  }

  function hasConflict(command: ShortcutCommandInfo): boolean {
    return command.effectiveKeybindings.some((binding) => findShortcutConflict(command.id, binding) !== null)
  }

  function matchesFilter(command: ShortcutCommandInfo): boolean {
    if (filter === 'modified') return isModified(command)
    if (filter === 'unassigned') return command.effectiveKeybindings.length === 0
    if (filter === 'conflicts') return hasConflict(command)
    return true
  }

  function isFilter(value: string): value is Filter {
    return (FILTERS as readonly string[]).includes(value)
  }

  let commands = $derived(
    getShortcutInfos()
      .filter(matchesSearch)
      .filter(matchesFilter)
      .sort(
        (a, b) => a.source.localeCompare(b.source) || a.group.localeCompare(b.group) || a.label.localeCompare(b.label)
      )
  )

  let grouped = $derived.by(() => {
    const buckets = new Map<string, ShortcutCommandInfo[]>()
    for (const command of commands) {
      const heading = command.source === 'editor' ? `Editor / ${command.group}` : command.group
      const existing = buckets.get(heading)
      if (existing) existing.push(command)
      else buckets.set(heading, [command])
    }
    return Array.from(buckets, ([heading, entries]) => ({ heading, entries }))
  })
</script>

<section class="flex flex-col gap-3 border-b border-edge px-5 py-4">
  <h3 class="text-md font-semibold text-bright">Keyboard Shortcuts</h3>
  <div class="flex items-center gap-2">
    <Input bind:value={search} placeholder="Search commands..." class="max-w-md" />
    <TabsRoot
      value={filter}
      onValueChange={(value) => {
        if (isFilter(value)) filter = value
      }}
    >
      <TabsList>
        {#each FILTERS as option (option)}
          <TabsTrigger value={option}>{FILTER_LABELS[option]}</TabsTrigger>
        {/each}
      </TabsList>
    </TabsRoot>
  </div>
</section>

<section class="px-5 py-2">
  {#if grouped.length === 0}
    <div class="py-8 text-sm text-subtle">No commands match.</div>
  {:else}
    {#each grouped as group (group.heading)}
      <div class="py-2">
        <h4 class="px-1 py-2 text-xs font-medium tracking-wide text-subtle uppercase">{group.heading}</h4>
        <div class="divide-y divide-edge border-y border-edge">
          {#each group.entries as command (command.id)}
            <div class="flex items-center gap-4 py-2">
              <div class="min-w-0 flex-1">
                <div class="flex items-center gap-2">
                  <span class="truncate text-sm text-fg">{command.label}</span>
                  {#if command.dangerous}
                    <Badge variant="warning">Confirm</Badge>
                  {/if}
                </div>
                <div class="truncate font-mono text-2xs text-subtle">{command.id}</div>
              </div>

              <div class="flex min-w-52 flex-wrap justify-end gap-1.5">
                {#if command.effectiveKeybindings.length > 0}
                  {#each command.effectiveKeybindings as binding (binding)}
                    {#if findShortcutConflict(command.id, binding)}
                      <span class="rounded-md border border-warning/40 bg-warning-bg px-1.5 py-1">
                        <ShortcutPreview spec={binding} />
                      </span>
                    {:else}
                      <ShortcutPreview spec={binding} />
                    {/if}
                  {/each}
                {:else}
                  <span class="text-xs text-subtle italic">Unassigned</span>
                {/if}
              </div>

              <div class="flex w-28 shrink-0 justify-end gap-1.5">
                <Button size="xs" variant="ghost" onclick={() => (rebindTarget = command)}>
                  {command.effectiveKeybindings.length > 0 ? 'Edit' : 'Add'}
                </Button>
                <Button
                  size="xs"
                  variant="ghost"
                  disabled={!isModified(command)}
                  onclick={() => clearShortcutOverride(command.id)}
                >
                  Reset
                </Button>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/each}
  {/if}
</section>

{#if rebindTarget}
  <RebindDialog
    open={true}
    commandId={rebindTarget.id}
    commandLabel={rebindTarget.label}
    defaultKeybindings={rebindTarget.defaultKeybindings}
    onClose={() => (rebindTarget = null)}
  />
{/if}
