<script lang="ts">
  import { parseStashMessage } from '$lib/features/git/gitRefs'
  import ChangedFilesTree from '$lib/features/git/ChangedFilesTree.svelte'
  import { openStashDiff } from '$lib/features/workbench/surfaces/diff/service.svelte'
  import { timeAgo, formatFullDate } from '$lib/utils/date'
  import * as Tooltip from '$lib/components/ui/tooltip'
  import * as ContextMenu from '$lib/components/ui/context-menu'
  import * as DropdownMenu from '$lib/components/ui/dropdown-menu'
  import { Button, IconButton, iconButtonVariants } from '$lib/components/ui/button'
  import { sidebarRowVariants } from '$lib/components/ui/sidebar-row'
  import {
    ClockIcon,
    CopyIcon,
    GitBranchIcon,
    GitCommitIcon,
    MoreHorizontalIcon,
    PackageIcon,
    PackageOpenIcon,
    Play,
    Trash2
  } from '$lib/icons/lucideExports'
  import { GRAPH_COLORS } from '$lib/features/git/graph'
  import {
    applyStash,
    branchFromStash,
    copyToClipboard,
    clearStashes,
    dropStash,
    popLatestStash,
    popStash,
    promptStash
  } from '$lib/features/git/actions.svelte'
  import { getGitSummary, getStashes, loadStashes } from '$lib/features/git/state.svelte'

  let {
    folderPath,
    branchColorMap = new Map()
  }: {
    folderPath: string
    branchColorMap?: Map<string, string>
  } = $props()

  let stashes = $derived(getStashes(folderPath) ?? [])
  let summary = $derived(getGitSummary(folderPath))
  let expandedIndex = $state<number | null>(null)

  $effect(() => {
    void loadStashes(folderPath)
  })

  /** Look up a branch's color from the graph lane assignments. */
  function branchColor(branch: string): string {
    // Try exact match first, then check for origin/ prefix variants
    return (
      branchColorMap.get(branch) ??
      branchColorMap.get(`origin/${branch}`) ??
      branchColorMap.get(branch.replace(/^origin\//, '')) ??
      GRAPH_COLORS[0]
    )
  }

  function toggleStash(index: number) {
    if (expandedIndex === index) {
      expandedIndex = null
      return
    }
    expandedIndex = index
  }

  async function handlePop(index: number) {
    if (await popStash(folderPath, index)) expandedIndex = null
  }

  async function handleDrop(idx: number) {
    if ((await dropStash(folderPath, idx)) && expandedIndex === idx) expandedIndex = null
  }
</script>

<div data-slot="git-stashes" class="flex h-full min-h-0 flex-col overflow-hidden text-base">
  <div class="flex shrink-0 items-center gap-1.5 border-b border-edge px-2 py-1.5">
    <Button
      size="sm"
      variant="ghost"
      disabled={(summary?.staged_count ?? 0) + (summary?.unstaged_count ?? 0) === 0}
      onclick={() => void promptStash(folderPath, { kind: 'all', includeUntracked: false })}
    >
      <PackageIcon size={14} />
      Stash…
    </Button>
    <div class="flex-1"></div>
    <DropdownMenu.Root>
      <DropdownMenu.Trigger class={iconButtonVariants()} aria-label="Stash Actions">
        <MoreHorizontalIcon size={13} />
      </DropdownMenu.Trigger>
      <DropdownMenu.Content align="end">
        <DropdownMenu.Item
          disabled={(summary?.changes.length ?? 0) === 0}
          onSelect={() => void promptStash(folderPath, { kind: 'all', includeUntracked: true })}
        >
          <PackageIcon size={14} />
          <span>Stash (Include Untracked)…</span>
        </DropdownMenu.Item>
        <DropdownMenu.Item
          disabled={(summary?.staged_count ?? 0) === 0}
          onSelect={() => void promptStash(folderPath, { kind: 'staged' })}
        >
          <PackageIcon size={14} />
          <span>Stash Staged…</span>
        </DropdownMenu.Item>
        <DropdownMenu.Separator />
        <DropdownMenu.Item disabled={stashes.length === 0} onSelect={() => void applyStash(folderPath, 0)}>
          <PackageOpenIcon size={14} />
          <span>Apply Latest Stash</span>
        </DropdownMenu.Item>
        <DropdownMenu.Item disabled={stashes.length === 0} onSelect={() => void popLatestStash(folderPath)}>
          <Play size={14} />
          <span>Pop Latest Stash</span>
        </DropdownMenu.Item>
        <DropdownMenu.Separator />
        <DropdownMenu.Item destructive disabled={stashes.length === 0} onSelect={() => void clearStashes(folderPath)}>
          <Trash2 size={14} />
          <span>Drop All Stashes…</span>
        </DropdownMenu.Item>
      </DropdownMenu.Content>
    </DropdownMenu.Root>
  </div>
  {#if stashes.length === 0}
    <div class="px-2.5 py-2 text-sm text-subtle">No stashes.</div>
  {:else}
    <Tooltip.Provider delayDuration={400} skipDelayDuration={100}>
      <div class="min-h-0 flex-1 overflow-y-auto">
        {#each stashes as stash (stash.index)}
          {@const isExpanded = expandedIndex === stash.index}
          {@const parsed = parseStashMessage(stash.message)}
          {@const color = parsed.branch ? branchColor(parsed.branch) : GRAPH_COLORS[0]}
          {@const fileCount = stash.files.length}
          {@const totalAdds = stash.files.reduce((s, f) => s + (f.additions ?? 0), 0)}
          {@const totalDels = stash.files.reduce((s, f) => s + (f.deletions ?? 0), 0)}

          <ContextMenu.Root>
            <ContextMenu.Trigger class="contents">
              <!-- Row is a wrapping div so the action buttons can sit as siblings
               to the tooltip-triggering row button. Nesting <button>s inside
               <button>s breaks keyboard/pointer semantics. -->
              <div class="{sidebarRowVariants({ variant: 'leaf', pressed: isExpanded })} group">
                <Tooltip.Root>
                  <Tooltip.Trigger
                    class="flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 border-none bg-transparent px-0 text-left focus-visible:shadow-focus-ring focus-visible:outline-none"
                    onclick={() => toggleStash(stash.index)}
                  >
                    <span class="shrink-0 rounded bg-accent-bg px-1 py-px font-mono text-2xs text-accent">
                      {stash.index}
                    </span>
                    {#if parsed.branch}
                      <span
                        class="inline-flex max-w-24 shrink-0 items-center truncate rounded px-1 py-px font-mono text-2xs leading-tight"
                        style="background: color-mix(in srgb, {color} 20%, transparent); color: {color}"
                      >
                        {parsed.branch}
                      </span>
                    {/if}
                    <span class="min-w-0 flex-1 truncate text-sm text-fg">
                      {parsed.label}
                    </span>
                  </Tooltip.Trigger>
                  <Tooltip.Content class="max-w-80" sideOffset={6} side="right" align="center">
                    <div class="flex flex-col gap-2 py-0.5">
                      <div class="flex items-center gap-1 text-sm">
                        <ClockIcon size={10} class="text-subtle" />
                        <span class="text-muted">{timeAgo(stash.date)}</span>
                        <span class="text-subtle">({formatFullDate(stash.date)})</span>
                      </div>
                      <p class="text-sm leading-snug text-fg">{stash.message}</p>
                      {#if fileCount > 0}
                        <p class="text-xs">
                          <span class="text-muted">{fileCount} file{fileCount !== 1 ? 's' : ''},</span>
                          {' '}
                          <span class="text-success">{totalAdds} insertions(+)</span>
                          <span class="text-muted">,</span>
                          {' '}
                          <span class="text-danger">{totalDels} deletions(-)</span>
                        </p>
                      {/if}
                      {#if parsed.branch}
                        <div class="flex items-center gap-1 text-xs">
                          <span
                            class="inline-block h-1 w-1 shrink-0 rounded-full"
                            style="background: {color}"
                            aria-hidden="true"
                          ></span>
                          <span class="font-mono" style="color: {color}">{parsed.branch}</span>
                        </div>
                      {/if}
                    </div>
                  </Tooltip.Content>
                </Tooltip.Root>

                <span class="shrink-0 text-2xs text-subtle group-hover:hidden">
                  {timeAgo(stash.date)}
                </span>
                <div class="hidden shrink-0 items-center gap-0.5 group-hover:flex">
                  <IconButton tooltip="Pop (apply + remove)" tooltipSide="left" onclick={() => handlePop(stash.index)}>
                    <Play size={12} />
                  </IconButton>
                  <IconButton
                    tooltip="Drop stash"
                    tooltipSide="left"
                    tone="danger"
                    onclick={() => handleDrop(stash.index)}
                  >
                    <Trash2 size={12} />
                  </IconButton>
                </div>
              </div>
            </ContextMenu.Trigger>
            <ContextMenu.Content>
              <ContextMenu.Item onSelect={() => void openStashDiff(folderPath, stash.index, stash.message)}>
                <GitCommitIcon size={14} />
                <span>Open Changes</span>
              </ContextMenu.Item>
              <ContextMenu.Item onSelect={() => void applyStash(folderPath, stash.index)}>
                <PackageOpenIcon size={14} />
                <span>Apply Stash</span>
              </ContextMenu.Item>
              <ContextMenu.Item onSelect={() => void handlePop(stash.index)}>
                <Play size={14} />
                <span>Pop Stash</span>
              </ContextMenu.Item>
              <ContextMenu.Item onSelect={() => void branchFromStash(folderPath, stash.index)}>
                <GitBranchIcon size={14} />
                <span>Create Branch from Stash…</span>
              </ContextMenu.Item>
              <ContextMenu.Separator />
              <ContextMenu.Item destructive onSelect={() => void handleDrop(stash.index)}>
                <Trash2 size={14} />
                <span>Drop Stash…</span>
              </ContextMenu.Item>
              <ContextMenu.Separator />
              <ContextMenu.Item
                onSelect={() => void copyToClipboard(`stash@{${stash.index}}`, 'Copy stash name failed')}
              >
                <CopyIcon size={14} />
                <span>Copy Stash Name</span>
              </ContextMenu.Item>
            </ContextMenu.Content>
          </ContextMenu.Root>

          {#if isExpanded}
            <div class="border-t border-edge/30 bg-surface py-1">
              <ChangedFilesTree
                files={stash.files}
                open={(path) => openStashDiff(folderPath, stash.index, stash.message, path)}
              />
            </div>
          {/if}
        {/each}
      </div>
    </Tooltip.Provider>
  {/if}
</div>
