<script lang="ts">
  import StageView from '$lib/components/layout/StageView.svelte'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { Button } from '$lib/components/ui/button'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { MagicCard } from '$lib/components/ui/magic-card'
  import { Separator } from '$lib/components/ui/separator'
  import { localHostLabel, openBrowser } from '$lib/features/browser/state.svelte'
  import { splitShortcut } from '$lib/features/command-palette/shortcuts/spec'
  import { getCommandShortcut } from '$lib/features/command-palette/shortcuts/registry.svelte'
  import { getGitSummary } from '$lib/features/git/state.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getNixStatus } from '$lib/features/settings/state/nix.svelte'
  import { allProviders, directOptions, type ProviderMeta } from '$lib/features/sessions/providers/catalog'
  import ProviderIcon from '$lib/features/sessions/providers/ProviderIcon.svelte'
  import { getConnectedProviders, getProvidersLoading } from '$lib/features/sessions/providers/state.svelte'
  import { startSession } from '$lib/features/sessions/service.svelte'
  import { openTaskTab } from '$lib/features/tasks/service.svelte'
  import { getTasksReactive } from '$lib/features/tasks/state.svelte'
  import { createUntitledTextSurface } from '$lib/features/workbench/surfaces/text/service.svelte'
  import { MONO_FONT_FAMILY } from '$lib/fonts'
  import LucideIcon from '$lib/icons/LucideIcon.svelte'
  import MaskIcon from '$lib/icons/MaskIcon.svelte'
  import nixosUrl from '$lib/assets/nixos.svg?url'
  import {
    ArrowDown,
    ArrowUp,
    ArrowUpRightIcon,
    ChevronRight,
    FilePlusCornerIcon,
    GitBranchIcon,
    MonitorIcon,
    Play,
    ServerIcon
  } from '$lib/icons/lucideExports'
  import { platform } from '$lib/platform'
  import type { TaskDefinition } from '$lib/types/backend'
  import { basename, splitRemotePath } from '$lib/utils/paths'
  import { getPathColor } from '$lib/utils/pathColor'

  let { folderPath }: { folderPath: string } = $props()

  let remote = $derived(splitRemotePath(folderPath))
  let folderName = $derived(basename(remote?.path ?? folderPath) || '/')
  let git = $derived(getGitSummary(folderPath))
  // `changes` lists a path twice when it is both staged and unstaged.
  let changedCount = $derived(git ? new Set(git.changes.map((change) => change.path)).size : 0)
  let nix = $derived(getNixStatus(folderPath))
  let tasks = $derived(getTasksReactive(folderPath))
  let providersLoading = $derived(getProvidersLoading())
  let providerMap = $derived(new Map(getConnectedProviders(folderPath).map((p) => [p.id, p])))
  // Stagger slot after the direct row; New File always reserves one so tasks never jump.
  const tasksDelay = 0.1 + (allProviders.length + directOptions.length + 2) * 0.08
  // New File starts no session; it wears Terminal's neutral card.
  const newFile: ProviderMeta = {
    id: 'new-file',
    label: 'New File',
    icon: FilePlusCornerIcon,
    textLabel: 'New File',
    textFont: MONO_FONT_FAMILY,
    gradientFrom: 'var(--color-muted)',
    gradientTo: 'var(--color-edge-strong)'
  }
  let openShortcut = $derived(splitShortcut(getCommandShortcut('open-folder')))

  function handleSelect(provider: ProviderMeta) {
    startSession(folderPath, provider.id, provider.id === 'terminal' ? 'Terminal' : `${provider.label} session`)
  }

  function runTask(task: TaskDefinition) {
    void openTaskTab(folderPath, task).catch((error) => notify.error('Run task failed', getErrorMessage(error)))
  }
</script>

{#snippet card(option: ProviderMeta, delay: number, onselect: (() => void) | null, version: string | null)}
  {@const enabled = onselect !== null}
  <BlurFade {delay}>
    <MagicCard
      class="w-full"
      gradientFrom={option.gradientFrom}
      gradientTo={option.gradientTo}
      disabled={!enabled}
      onclick={() => onselect?.()}
    >
      <div class="flex items-center gap-3 p-3">
        <ProviderIcon icon={option.icon} size={32} class="text-muted" />
        <div class="flex min-w-0 flex-col gap-0.5">
          {#if option.textIcon && option.textAspect}
            <MaskIcon
              src={option.textIcon}
              width={Math.round(14 * option.textAspect)}
              height={14}
              label={option.label}
              class="self-start text-fg transition-colors group-hover:text-bright"
            />
          {:else}
            <span
              class="truncate text-md leading-tight font-medium text-fg transition-colors group-hover:text-bright"
              style:font-family={option.textFont ?? 'inherit'}>{option.textLabel ?? option.label}</span
            >
          {/if}
          {#if !enabled}
            <span class="text-2xs text-subtle italic">{providersLoading ? 'Detecting...' : 'Not detected'}</span>
          {:else if version}
            <span class="truncate font-mono text-2xs text-subtle">{version}</span>
          {/if}
        </div>
      </div>
    </MagicCard>
  </BlurFade>
{/snippet}

{#snippet providerCard(provider: ProviderMeta, delay: number)}
  {@const status = providerMap.get(provider.id)}
  {@render card(provider, delay, status ? () => handleSelect(provider) : null, status?.version ?? null)}
{/snippet}

{#snippet divider(label: string, delay: number)}
  <BlurFade {delay}>
    <div class="my-4 flex items-center gap-4">
      <Separator class="flex-1" />
      <span class="text-sm text-muted">{label}</span>
      <Separator class="flex-1" />
    </div>
  </BlurFade>
{/snippet}

<StageView>
  {#snippet footer()}
    <Button variant="outline" size="sm" class="text-muted" onclick={() => openBrowser()}>
      <ArrowUpRightIcon size={14} class="shrink-0" />
      Open Another Project…
      <KbdGroup class="ml-1">
        {#each openShortcut as part (part)}<Kbd class="h-5 min-w-5 rounded-sm px-1">{part}</Kbd>{/each}
      </KbdGroup>
    </Button>
  {/snippet}

  <BlurFade delay={0.05} duration={0.5} offset={10}>
    <header class="mb-8 flex flex-col items-center gap-1 text-center">
      <!-- The chevron hangs left of the name so the name alone is centered. -->
      <h2 class="relative max-w-full text-3xl text-bright">
        <ChevronRight
          size={28}
          class="absolute top-1/2 right-full -translate-y-1/2"
          style="color: {getPathColor(folderPath)}"
          aria-hidden="true"
        />
        <span class="block truncate">{folderName}</span>
      </h2>
      <p class="max-w-full truncate font-mono text-sm text-subtle" title={folderPath}>
        {remote?.path ?? folderPath}
      </p>
      <div class="mt-3 flex flex-wrap items-center justify-center gap-x-4 gap-y-1 text-sm text-muted">
        <span class="inline-flex items-center gap-1.5">
          {#if remote}
            <ServerIcon size={12} class="shrink-0" /><span class="font-mono">{remote.server}</span>
          {:else}
            <MonitorIcon size={12} class="shrink-0" />{localHostLabel()}
          {/if}
        </span>
        {#if git?.is_repo}
          <span class="inline-flex items-center gap-1.5 font-mono">
            <GitBranchIcon size={12} class="shrink-0" />{git.branch ?? 'Detached'}
            {#if git.ahead}<span class="inline-flex items-center" title="Commits Ahead"
                ><ArrowUp size={12} />{git.ahead}</span
              >{/if}
            {#if git.behind}<span class="inline-flex items-center" title="Commits Behind"
                ><ArrowDown size={12} />{git.behind}</span
              >{/if}
          </span>
          <span class="inline-flex items-center gap-1.5">
            <span class="size-1.5 shrink-0 rounded-full {changedCount ? 'bg-warning' : 'bg-success'}"></span>
            {changedCount ? `${changedCount} Changed` : 'Clean'}
          </span>
        {/if}
        {#if nix}
          <span class="inline-flex items-center gap-1.5">
            <MaskIcon src={nixosUrl} width={12} label="Nix" class={nix.tone} />{nix.label}
          </span>
        {/if}
      </div>
    </header>
  </BlurFade>

  <!-- Agent CLIs -->
  <div class="grid grid-cols-2 gap-2.5">
    {#each allProviders as provider, i (provider.id)}
      {@render providerCard(provider, 0.1 + i * 0.08)}
    {/each}
  </div>

  {@render divider('or', 0.1 + allProviders.length * 0.08)}

  <!-- Direct options -->
  <div class="grid grid-cols-2 gap-2.5">
    {#each directOptions as provider, i (provider.id)}
      {@render providerCard(provider, 0.1 + (allProviders.length + 1 + i) * 0.08)}
    {/each}
    {#if platform.native}
      {@render card(
        newFile,
        0.1 + (allProviders.length + directOptions.length + 1) * 0.08,
        () => createUntitledTextSurface(folderPath),
        null
      )}
    {/if}
  </div>

  {#if tasks.length > 0}
    {@render divider('tasks', tasksDelay)}
    <BlurFade delay={tasksDelay + 0.08}>
      <div class="flex flex-wrap justify-center gap-2">
        {#each tasks as task (task.id)}
          <Button size="sm" title={task.command} onclick={() => runTask(task)}>
            {#if task.icon}
              <LucideIcon name={task.icon} size={14} class="shrink-0" />
            {:else}
              <Play size={14} class="shrink-0" />
            {/if}
            {task.label}
          </Button>
        {/each}
      </div>
    </BlurFade>
  {/if}
</StageView>
