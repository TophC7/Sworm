<!--
  @component
  NewTabMenu — the `+` button at the end of the title-bar tab strip.
  Opens Terminal / New File / one row per connected agent provider in the
  active tab's folder, or opens another folder. With no active tab there is no folder to target,
  so the button opens the folder picker instead.
-->

<script lang="ts">
  import { IconButton, iconButtonVariants } from '$lib/components/ui/button'
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuSeparator,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import { createSession, openFolderPicker } from '$lib/features/app-actions/actions.svelte'
  import { allProviders } from '$lib/features/sessions/providers/catalog'
  import { getConnectedProviders } from '$lib/features/sessions/providers/state.svelte'
  import { startSession } from '$lib/features/sessions/service.svelte'
  import { getActiveFolderPath } from '$lib/features/workbench/state.svelte'
  import { createUntitledTextSurface } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { FilePlusIcon, FolderOpen, Plus, TerminalIcon } from '$lib/icons/lucideExports'

  let folderPath = $derived(getActiveFolderPath())
  let connectedAgents = $derived.by(() => {
    const connected = new Set(getConnectedProviders(folderPath).map((p) => p.id))
    return allProviders.filter((p) => connected.has(p.id))
  })

  const buttonClass = 'sticky right-0 ml-0.5 shrink-0 bg-surface'
</script>

{#if folderPath}
  {@const folder = folderPath}
  <DropdownMenuRoot>
    <DropdownMenuTrigger
      aria-label="New tab"
      title="New tab"
      class={iconButtonVariants({ size: 'md', class: buttonClass })}
    >
      <Plus size={14} />
    </DropdownMenuTrigger>
    <DropdownMenuContent align="start" sideOffset={4}>
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
      <DropdownMenuItem onclick={() => void openFolderPicker()}>
        <FolderOpen size={14} class="shrink-0 text-muted" />
        <span>Open Folder…</span>
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenuRoot>
{:else}
  <IconButton size="md" tooltip="Open folder" class={buttonClass} onclick={() => void openFolderPicker()}>
    <Plus size={14} />
  </IconButton>
{/if}
