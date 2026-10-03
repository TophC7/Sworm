<script lang="ts">
  import { Button, iconButtonVariants } from '$lib/components/ui/button'
  import {
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuRoot,
    DropdownMenuTrigger
  } from '$lib/components/ui/dropdown-menu'
  import { providerById, type ProviderIconSource } from '$lib/features/sessions/providers/catalog'
  import ProviderIcon from '$lib/features/sessions/providers/ProviderIcon.svelte'
  import { MoreHorizontalIcon, Play } from '$lib/icons/lucideExports'
  import type { WorkbenchInfo } from '$lib/types/backend'
  import { cn } from '$lib/utils/cn'
  import { timeAgo } from '$lib/utils/date'
  import { workbenchHref } from './workbenchLink'
  import { workbenchTitle } from '$lib/features/browser/places.svelte'
  import { onDestroy } from 'svelte'

  let {
    workbench,
    highlighted,
    closing,
    error,
    onOpen,
    onTakeOver,
    onClose
  }: {
    workbench: WorkbenchInfo
    /** A hovered project card is open in this workbench. */
    highlighted: boolean
    closing: boolean
    error: string | undefined
    onOpen?: () => void
    onTakeOver: () => void
    onClose: () => void
  } = $props()

  // Open elsewhere: the row never navigates. Clicking it flashes Take Over instead of asking in a dialog.
  let hinting = $state(false)
  let hintTimer: ReturnType<typeof setTimeout> | undefined
  function hintTakeOver(): void {
    clearTimeout(hintTimer)
    hinting = false
    requestAnimationFrame(() => {
      hinting = true
      hintTimer = setTimeout(() => (hinting = false), 900)
    })
  }
  onDestroy(() => clearTimeout(hintTimer))

  interface RunChip {
    label: string
    icon: ProviderIconSource | null
    task: boolean
    count: number
  }

  // Identical runs collapse into one chip with a count.
  let chips = $derived.by(() => {
    const byKey = new Map<string, RunChip>()
    for (const run of workbench.running) {
      const key = run.kind === 'session' ? `session:${run.provider_id}` : `task:${run.task_id}`
      const chip = byKey.get(key)
      if (chip) {
        chip.count += 1
      } else if (run.kind === 'session') {
        const provider = providerById[run.provider_id]
        byKey.set(key, {
          label: provider?.label ?? run.provider_id,
          icon: provider?.icon ?? null,
          task: false,
          count: 1
        })
      } else {
        byKey.set(key, { label: run.task_id, icon: null, task: true, count: 1 })
      }
    }
    return [...byKey.values()]
  })
  let title = $derived(workbenchTitle(workbench))
</script>

{#snippet rowContent()}
  <div class="min-w-0 flex-1">
    <div class="truncate text-base text-bright">{title}</div>
    <div class="flex min-w-0 items-center gap-3 overflow-hidden text-xs text-muted">
      {#each chips as chip (chip.label + chip.task)}
        <span class="flex shrink-0 items-center gap-1">
          {#if chip.icon}
            <ProviderIcon icon={chip.icon} size={12} />
          {:else if chip.task}
            <Play size={10} class="text-subtle" />
          {/if}
          <span class={cn(chip.task && 'font-mono')}>{chip.label}</span>
          {#if chip.count > 1}
            <span class="text-subtle">×{chip.count}</span>
          {/if}
        </span>
      {:else}
        <span class="text-subtle">Nothing running</span>
      {/each}
    </div>
  </div>
  {#if workbench.connected}
    <span class={cn('duration-med shrink-0 text-xs transition-colors', hinting ? 'text-bright' : 'text-subtle')}
      >{workbench.client ? `Active in ${workbench.client}` : 'Active elsewhere'}</span
    >
  {/if}
  <span class="w-16 shrink-0 text-right text-xs text-subtle">
    {closing ? 'Closing…' : timeAgo(workbench.last_seen_at)}
  </span>
{/snippet}
<li class="flex flex-col">
  <div
    class={cn(
      'flex items-center gap-2 rounded-lg py-1.5 pr-1.5 pl-3 hover:bg-surface',
      highlighted && 'bg-accent-bg hover:bg-accent-bg'
    )}
  >
    {#if workbench.connected || onOpen}
      <button
        type="button"
        onclick={workbench.connected ? hintTakeOver : onOpen}
        class="flex min-w-0 flex-1 cursor-pointer items-center gap-3 rounded-sm text-left focus-visible:shadow-focus-ring focus-visible:outline-none"
      >
        {@render rowContent()}
      </button>
      {#if workbench.connected}
        <Button
          size="xs"
          variant={hinting ? 'accent' : 'outline'}
          class="duration-med shrink-0"
          disabled={closing}
          onclick={onTakeOver}>Take Over</Button
        >
      {/if}
    {:else}
      <a
        href={workbenchHref(workbench.id)}
        data-sveltekit-reload
        class="flex min-w-0 flex-1 items-center gap-3 rounded-sm focus-visible:shadow-focus-ring focus-visible:outline-none"
      >
        {@render rowContent()}
      </a>
    {/if}
    <DropdownMenuRoot>
      <DropdownMenuTrigger
        aria-label="Workbench actions"
        title="Workbench actions"
        disabled={closing}
        class={iconButtonVariants({ size: 'md' })}
      >
        <MoreHorizontalIcon size={14} />
      </DropdownMenuTrigger>
      <DropdownMenuContent align="end" sideOffset={4}>
        <DropdownMenuItem destructive onclick={onClose}>Close Workbench…</DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenuRoot>
  </div>
  {#if error}
    <p role="alert" class="px-3 pb-1.5 text-sm break-words text-danger-bright">{error}</p>
  {/if}
</li>
