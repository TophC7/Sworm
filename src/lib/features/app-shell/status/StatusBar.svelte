<script lang="ts">
  import { getActiveFolderPath, getTabs } from '$lib/features/workbench/state.svelte'
  import { isProcessLive } from '$lib/features/workbench/model'
  import { StatusChip, statusChipVariants } from '$lib/components/ui/status-chip'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import NixEnvIndicator from '$lib/features/app-shell/status/NixEnvIndicator.svelte'
  import NotificationsButton from '$lib/features/notifications/NotificationsButton.svelte'
  import StatusBarBranchPopover from '$lib/features/app-shell/status/StatusBarBranchPopover.svelte'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { isBrowserOpen, toggleBrowser } from '$lib/features/browser/state.svelte'
  import StatusBarAppInfo from '$lib/features/app-shell/status/StatusBarAppInfo.svelte'
  import {
    ensureSettingsDiagnosticsListener,
    getSettingsDiagnostics,
    refreshSettingsDiagnostics
  } from '$lib/features/settings/state/diagnostics.svelte'
  import { AlertTriangle, FolderOpenIcon } from '$lib/icons/lucideExports'
  import { folderCrumbs, splitRemotePath } from '$lib/utils/paths'
  import { remoteDotClass } from '$lib/features/remotes/remoteDot'
  import { platform } from '$lib/platform'
  import { openRemoteManager } from '$lib/features/remotes/state.svelte'
  import { getRemoteStatus, refreshRemoteStatus, watchRemoteStatuses } from '$lib/features/remotes/status.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import type { RemoteStatus } from '$lib/types/backend'
  import type { Snippet } from 'svelte'

  let { connectionStatus }: { connectionStatus?: Snippet } = $props()

  let folderPath = $derived(getActiveFolderPath())
  let remote = $derived(folderPath ? splitRemotePath(folderPath) : null)
  let remoteServer = $derived(remote?.server ?? null)
  let remoteStatus = $derived(remoteServer ? getRemoteStatus(remoteServer) : null)

  $effect(() => {
    const server = remoteServer
    const stop = watchRemoteStatuses()
    if (server) void refreshRemoteStatus(server)
    return stop
  })
  let sharedAgentCount = $derived(
    getTabs().filter(
      (t) =>
        t.kind === 'session' && t.folderPath === folderPath && t.providerId !== 'terminal' && isProcessLive(t.status)
    ).length
  )
  let remoteState = $derived<RemoteStatus['state'] | 'checking'>(remoteStatus?.state ?? 'checking')
  let settingsDiagnostics = $derived(getSettingsDiagnostics())

  $effect(() => {
    ensureSettingsDiagnosticsListener()
    const requestedFolderPath = folderPath
    let disposed = false
    void refreshSettingsDiagnostics(requestedFolderPath ?? undefined).catch((error) => {
      if (!disposed && requestedFolderPath === getActiveFolderPath()) {
        notify.error('Failed to load settings diagnostics', getErrorMessage(error))
      }
    })
    return () => {
      disposed = true
    }
  })
</script>

<footer
  class="flex min-h-6 shrink-0 items-center justify-between gap-3 border-t border-edge bg-surface px-1 py-0.5 text-xs"
>
  <div class="flex items-center gap-1">
    <StatusBarAppInfo />
    {#if connectionStatus}{@render connectionStatus()}{/if}
    {#if remoteServer && platform.native}
      <StatusChip
        onclick={openRemoteManager}
        title={remoteStatus?.last_error ?? `${remoteServer}: ${remoteState}`}
        aria-label="Manage remote {remoteServer}: {remoteState}"
      >
        <span class="size-1.5 shrink-0 rounded-full {remoteDotClass(remoteState)}"></span>
        <span class="font-mono">{remoteServer}</span>
      </StatusChip>
    {/if}
    {#if folderPath}
      <StatusChip
        data-browser-toggle="true"
        aria-label="Browse Folder"
        aria-haspopup="dialog"
        aria-expanded={isBrowserOpen()}
        title={folderPath}
        onclick={() => toggleBrowser({ path: folderPath })}
        class="max-w-[min(32rem,40vw)]"
      >
        <FolderOpenIcon size={10} class="shrink-0" />
        <span class="truncate">{folderCrumbs(remote?.path ?? folderPath)}</span>
      </StatusChip>
      <StatusBarBranchPopover {folderPath} />
      <NixEnvIndicator {folderPath} />
    {/if}
  </div>

  <div class="flex items-center gap-1">
    {#if settingsDiagnostics.length > 0}
      <TooltipRoot>
        <TooltipTrigger class={statusChipVariants({ tone: 'warning' })}>
          <AlertTriangle size={10} />
          {settingsDiagnostics.length} settings
        </TooltipTrigger>
        <TooltipContent class="max-w-md">
          <div class="space-y-1 text-left">
            <div class="text-xs font-medium text-warning-bright">Settings diagnostics</div>
            {#each settingsDiagnostics.slice(0, 5) as diagnostic}
              <div class="font-mono text-2xs text-muted">
                {diagnostic.origin === 'host' ? 'host ' : ''}{diagnostic.layer}: {diagnostic.path}{diagnostic.pointer
                  ? ` ${diagnostic.pointer}`
                  : ''}
                <span class="font-sans text-warning-bright">{diagnostic.message}</span>
              </div>
            {/each}
            {#if settingsDiagnostics.length > 5}
              <div class="text-2xs text-subtle">+{settingsDiagnostics.length - 5} more</div>
            {/if}
          </div>
        </TooltipContent>
      </TooltipRoot>
    {/if}

    {#if sharedAgentCount > 1}
      <TooltipRoot>
        <TooltipTrigger class={statusChipVariants({ tone: 'warning' })}>
          <AlertTriangle size={10} /> shared
        </TooltipTrigger>
        <TooltipContent>Multiple agents share this folder's working tree</TooltipContent>
      </TooltipRoot>
    {/if}
    <NotificationsButton />
  </div>
</footer>
