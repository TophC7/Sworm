<script lang="ts">
  import { getActiveFolderPath, getTabs } from '$lib/features/workbench/state.svelte'
  import { isProcessLive } from '$lib/features/workbench/model'
  import { StatusChip, statusChipVariants } from '$lib/components/ui/status-chip'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import NixEnvIndicator from '$lib/features/app-shell/status/NixEnvIndicator.svelte'
  import NotificationsButton from '$lib/features/notifications/NotificationsButton.svelte'
  import StatusBarBranchPopover from '$lib/features/app-shell/status/StatusBarBranchPopover.svelte'
  import { isFolderSwitcherOpen, toggleFolderSwitcher } from '$lib/features/folders/switcher.svelte'
  import StatusBarAppInfo from '$lib/features/app-shell/status/StatusBarAppInfo.svelte'
  import {
    ensureSettingsDiagnosticsListener,
    getSettingsDiagnostics,
    refreshSettingsDiagnostics
  } from '$lib/features/settings/state/diagnostics.svelte'
  import { AlertTriangle, FolderOpen } from '$lib/icons/lucideExports'
  import { folderCrumbs, splitRemotePath } from '$lib/utils/paths'
  import { cn } from '$lib/utils/cn'
  import { platform, requireNative } from '$lib/platform'
  import { openRemoteManager } from '$lib/features/remotes/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import type { RemoteStatus } from '$lib/types/backend'

  let folderPath = $derived(getActiveFolderPath())
  let remote = $derived(folderPath ? splitRemotePath(folderPath) : null)
  let remoteServer = $derived(remote?.server ?? null)
  let remoteStatus = $state<RemoteStatus | null>(null)

  $effect(() => {
    const server = remoteServer
    remoteStatus = null
    if (!server || !platform.capabilities.remoteHosts) return
    let disposed = false
    let pending = false
    let revision = 0
    let unlisten: (() => void) | undefined
    async function refresh() {
      if (pending || disposed) return
      pending = true
      const requestedRevision = revision
      try {
        const status = await requireNative().remotes.status(server!)
        if (!disposed && revision === requestedRevision) remoteStatus = status
      } catch (error) {
        if (!disposed && revision === requestedRevision) {
          remoteStatus = { connected: false, state: 'error', last_error: getErrorMessage(error) }
        }
      } finally {
        pending = false
      }
    }
    void requireNative()
      .remotes.onStatus((status) => {
        if (disposed || status.server !== server) return
        revision++
        remoteStatus = status
      })
      .then((stop) => {
        if (disposed) stop()
        else unlisten = stop
      })
      .catch((error) => console.error('Remote status listener failed:', error))
      // Fetch after subscribing so no transition lands between the two.
      .finally(() => void refresh())
    return () => {
      disposed = true
      unlisten?.()
    }
  })
  let sharedAgentCount = $derived(
    getTabs().filter(
      (t) =>
        t.kind === 'session' && t.folderPath === folderPath && t.providerId !== 'terminal' && isProcessLive(t.status)
    ).length
  )
  let remoteState = $derived(remoteStatus?.state ?? 'checking')
  let settingsDiagnostics = $derived(getSettingsDiagnostics())

  $effect(() => {
    ensureSettingsDiagnosticsListener()
    void refreshSettingsDiagnostics(folderPath ?? undefined)
  })
</script>

<footer
  class="flex min-h-6 shrink-0 items-center justify-between gap-3 border-t border-edge bg-surface px-1 py-0.5 text-xs"
>
  <div class="flex items-center gap-1">
    <StatusBarAppInfo />
    {#if remoteServer && platform.capabilities.remoteHosts}
      <StatusChip
        onclick={openRemoteManager}
        title={remoteStatus?.last_error ?? `${remoteServer}: ${remoteState}`}
        aria-label="Manage remote {remoteServer}: {remoteState}"
      >
        <span
          class={cn(
            'size-1.5 shrink-0 rounded-full',
            remoteState === 'connected'
              ? 'bg-success'
              : remoteState === 'error'
                ? 'bg-danger'
                : remoteState === 'reconnecting'
                  ? 'bg-warning'
                  : 'bg-muted'
          )}
        ></span>
        <span class="font-mono">{remoteServer}</span>
      </StatusChip>
    {/if}
    {#if folderPath}
      <StatusChip
        data-folder-switcher-toggle="true"
        aria-label="Switch folder"
        aria-haspopup="dialog"
        aria-expanded={isFolderSwitcherOpen()}
        title={folderPath}
        onclick={toggleFolderSwitcher}
        class="max-w-[min(32rem,40vw)]"
      >
        <FolderOpen size={10} class="shrink-0" />
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
