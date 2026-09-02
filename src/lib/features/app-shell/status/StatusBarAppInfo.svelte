<!--
  @component
  StatusBarAppInfo — circular app badge with a fastfetch-style runtime summary.
-->

<script lang="ts">
  import { PopoverContent, PopoverRoot, PopoverTrigger } from '$lib/components/ui/popover'
  import { IconButton, iconButtonVariants } from '$lib/components/ui/button'
  import { cn } from '$lib/utils/cn'
  import { backend } from '$lib/api/backend'
  import githubIconUrl from '$lib/icons/github.svg?url'
  import appIconUrl from '../../../../../src-tauri/icons/icon.svg?url'
  import { getTabs } from '$lib/features/workbench/state.svelte'
  import { isProcessLive } from '$lib/features/workbench/model'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import type { AppRuntimeInfo } from '$lib/types/backend'
  import { openUrl } from '@tauri-apps/plugin-opener'

  const REPOSITORY_URL = 'https://github.com/tophc7/sworm'

  let open = $state(false)
  let info = $state<AppRuntimeInfo | null>(null)
  let cpuPercent = $state<number | null>(null)
  let loadError = $state<string | null>(null)

  let tabs = $derived(getTabs())
  let folderCount = $derived(new Set(tabs.map((tab) => tab.folderPath)).size)
  let liveProcessCount = $derived(
    tabs.filter((tab) => (tab.kind === 'session' || tab.kind === 'task') && isProcessLive(tab.status)).length
  )

  function formatBytes(bytes: number | null | undefined): string {
    if (bytes == null) return 'Unavailable'
    const mebibytes = bytes / (1024 * 1024)
    return `${mebibytes.toFixed(mebibytes >= 100 ? 0 : 1)} MiB`
  }

  function formatCpuPercent(percent: number): string {
    return `${percent.toFixed(percent < 1 ? 2 : 1)}% total`
  }

  async function openRepository(): Promise<void> {
    try {
      await openUrl(REPOSITORY_URL)
      open = false
    } catch (error) {
      notify.error('Open GitHub failed', getErrorMessage(error))
    }
  }

  $effect(() => {
    if (!open) return

    let cancelled = false
    let timeout: number | undefined
    let previousAppCpuTime: number | null = null
    let previousSystemCpuTime: number | null = null
    cpuPercent = null

    const refresh = async () => {
      try {
        const next = await backend.app.runtimeInfo()
        if (cancelled) return
        if (
          previousAppCpuTime != null &&
          previousSystemCpuTime != null &&
          next.app_cpu_time_ticks != null &&
          next.system_cpu_time_ticks != null
        ) {
          const appDelta = next.app_cpu_time_ticks - previousAppCpuTime
          const systemDelta = next.system_cpu_time_ticks - previousSystemCpuTime
          if (appDelta >= 0 && systemDelta > 0) {
            cpuPercent = Math.min(100, (appDelta / systemDelta) * 100)
          }
        }
        previousAppCpuTime = next.app_cpu_time_ticks
        previousSystemCpuTime = next.system_cpu_time_ticks
        info = next
        loadError = null
      } catch (error) {
        if (!cancelled) loadError = getErrorMessage(error)
      } finally {
        if (!cancelled) timeout = window.setTimeout(() => void refresh(), 1_500)
      }
    }

    void refresh()
    return () => {
      cancelled = true
      if (timeout !== undefined) window.clearTimeout(timeout)
    }
  })
</script>

<PopoverRoot bind:open>
  <PopoverTrigger
    aria-label="App information"
    class={cn(
      iconButtonVariants({ size: 'sm' }),
      'rounded-full border border-edge bg-raised text-accent hover:border-accent/50 hover:bg-raised hover:text-accent-bright'
    )}
  >
    <img src={appIconUrl} alt="" class="size-4" />
  </PopoverTrigger>

  <PopoverContent class="w-80 overflow-hidden p-0" align="start" sideOffset={8}>
    <div class="flex items-center gap-3 border-b border-edge bg-surface/60 p-3">
      <div class="flex size-10 shrink-0 items-center justify-center rounded-xl border border-edge bg-ground">
        <img src={appIconUrl} alt="" class="size-8" />
      </div>
      <div class="min-w-0 flex-1">
        <div class="flex items-baseline gap-2">
          <span class="truncate text-md font-semibold text-bright">{info?.name ?? 'Sworm'}</span>
          <span class="font-mono text-xs text-subtle">v{info?.version ?? '…'}</span>
        </div>
        <div class="text-xs text-muted">Agentic Development Environment</div>
      </div>
      <IconButton ariaLabel="Open Sworm on GitHub" size="md" class="shrink-0" onclick={() => void openRepository()}>
        <span
          class="size-5 bg-current"
          style:mask-image={`url("${githubIconUrl}")`}
          style:mask-position="center"
          style:mask-repeat="no-repeat"
          style:mask-size="contain"
          aria-hidden="true"
        ></span>
      </IconButton>
    </div>

    <div class="p-3 font-mono text-xs">
      <div class="mb-2 text-2xs tracking-widest text-subtle uppercase">App stats</div>
      <dl class="grid grid-cols-[5.5rem_1fr] gap-x-3 gap-y-1">
        <dt class="text-accent">Tabs</dt>
        <dd class="text-fg">{tabs.length}</dd>
        <dt class="text-accent">Folders</dt>
        <dd class="text-fg">{folderCount}</dd>
        <dt class="text-accent">Live</dt>
        <dd class="text-fg">{liveProcessCount} processes</dd>
      </dl>

      <div class="mt-3 mb-2 text-2xs tracking-widest text-subtle uppercase">Resources</div>
      <dl class="grid grid-cols-[5.5rem_1fr] gap-x-3 gap-y-1">
        <dt class="text-success">CPU</dt>
        <dd class="text-fg" title="Sworm process tree as a share of total machine CPU capacity">
          {cpuPercent == null ? 'Sampling…' : formatCpuPercent(cpuPercent)}
        </dd>
        <dt class="text-success">Memory</dt>
        <dd class="text-fg">{formatBytes(info?.memory_bytes)}</dd>
        <dt class="text-success">Threads</dt>
        <dd class="text-fg">{info?.thread_count ?? 'Unavailable'}</dd>
        <dt class="text-success">Open FDs</dt>
        <dd class="text-fg">{info?.file_descriptor_count ?? 'Unavailable'}</dd>
      </dl>

      {#if loadError}
        <div class="mt-3 rounded-md border border-danger-border bg-danger-bg px-2 py-1.5 font-sans text-xs text-danger">
          {loadError}
        </div>
      {/if}

      <div class="mt-3 flex h-1.5 overflow-hidden rounded-full" aria-hidden="true">
        <span class="flex-1 bg-danger"></span>
        <span class="flex-1 bg-warning"></span>
        <span class="flex-1 bg-success"></span>
        <span class="flex-1 bg-accent"></span>
        <span class="flex-1 bg-pink"></span>
        <span class="flex-1 bg-peach"></span>
      </div>
    </div>
  </PopoverContent>
</PopoverRoot>
