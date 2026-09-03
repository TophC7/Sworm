<!--
  @component
  NewTabMenu — the `+` button at the end of the title-bar tab strip.
  Opens Terminal / New File / one row per connected agent provider in the
  active tab's folder, plus folder picker and recent folders.
-->

<script lang="ts">
  import { IconButton, iconButtonVariants } from '$lib/components/ui/button'
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuSeparator,
    DropdownMenuSub,
    DropdownMenuSubContent,
    DropdownMenuSubTrigger,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import { createSession, openFolderPicker } from '$lib/features/app-actions/actions.svelte'
  import { allProviders } from '$lib/features/sessions/providers/catalog'
  import { getConnectedProviders } from '$lib/features/sessions/providers/state.svelte'
  import { startSession } from '$lib/features/sessions/service.svelte'
  import { getActiveFolderPath, getRecentUnopenedFolders, openFolder } from '$lib/features/workbench/state.svelte'
  import { createUntitledTextSurface } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { basename } from '$lib/utils/paths'
  import { FilePlusIcon, FolderClockIcon, FolderOpen, Plus, TerminalIcon } from '$lib/icons/lucideExports'

  let folderPath = $derived(getActiveFolderPath())
  let recentFolders = $derived(getRecentUnopenedFolders())
  let hasMenu = $derived(Boolean(folderPath) || recentFolders.length > 0)
  let connectedAgents = $derived.by(() => {
    if (!folderPath) return []
    const connected = new Set(getConnectedProviders(folderPath).map((p) => p.id))
    return allProviders.filter((p) => connected.has(p.id))
  })
  let triggerLabel = $derived(folderPath ? 'New tab' : 'Open folder')

  const buttonClass = 'sticky right-0 ml-0.5 shrink-0 bg-surface'
</script>

{#if hasMenu}
  <DropdownMenuRoot>
    <DropdownMenuTrigger
      aria-label={triggerLabel}
      title={triggerLabel}
      class={iconButtonVariants({ size: 'md', class: buttonClass })}
    >
      <Plus size={14} />
    </DropdownMenuTrigger>
    <DropdownMenuContent align="start" sideOffset={4}>
      {#if folderPath}
        {@const folder = folderPath}
        <DropdownMenuItem onclick={() => startSession(folder, 'terminal', 'Terminal')}>
          <TerminalIcon size={14} class="shrink-0 text-muted" />
          <span>Terminal</span>
        </DropdownMenuItem>
        <DropdownMenuItem onclick={() => createUntitledTextSurface(folder)}>
          <FilePlusIcon size={14} class="shrink-0 text-muted" />
          <span>New File</span>
        </DropdownMenuItem>
        {#if connectedAgents.length > 0}
          <DropdownMenuSeparator />
          {#each connectedAgents as provider (provider.id)}
            <DropdownMenuItem onclick={() => createSession(provider.id, provider.label)}>
              <img src={provider.icon} alt="" width={14} height={14} class="shrink-0" />
              <span>{provider.label}</span>
            </DropdownMenuItem>
          {/each}
        {/if}
        <DropdownMenuSeparator />
      {/if}

      <DropdownMenuItem onclick={() => void openFolderPicker()}>
        <FolderOpen size={14} class="shrink-0 text-muted" />
        <span>Open Folder…</span>
      </DropdownMenuItem>
      {#if recentFolders.length > 0}
        <DropdownMenuSub>
          <DropdownMenuSubTrigger>
            <FolderClockIcon size={14} class="shrink-0 text-muted" />
            <span>Open Recent</span>
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent>
            {#each recentFolders as recentPath (recentPath)}
              <DropdownMenuItem onclick={() => void openFolder(recentPath)}>
                <span class="truncate" title={recentPath}>{basename(recentPath)}</span>
              </DropdownMenuItem>
            {/each}
          </DropdownMenuSubContent>
        </DropdownMenuSub>
      {/if}
    </DropdownMenuContent>
  </DropdownMenuRoot>
{:else}
  <IconButton size="md" tooltip="Open folder" class={buttonClass} onclick={() => void openFolderPicker()}>
    <Plus size={14} />
  </IconButton>
{/if}
