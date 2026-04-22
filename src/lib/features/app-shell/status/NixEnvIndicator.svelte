<script lang="ts">
  import { onMount } from 'svelte'
  import { getActiveProject } from '$lib/features/projects/state.svelte'
  import {
    detectNix,
    evaluateNix,
    getNixDetection,
    isNixEvaluating,
    selectNixFile,
    clearNix
  } from '$lib/features/settings/state/nix.svelte'
  import { loadProvidersForProject } from '$lib/features/sessions/providers/state.svelte'
  import { Badge } from '$lib/components/ui/badge'
  import {
    DropdownMenuRoot,
    DropdownMenuTrigger,
    DropdownMenuContent,
    DropdownMenuItem,
    DropdownMenuSeparator
  } from '$lib/components/ui/dropdown-menu'
  import { LoaderCircle, Check, X, CircleAlert } from '$lib/icons/lucideExports'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'

  let project = $derived(getActiveProject())
  let detection = $derived(project ? getNixDetection(project.id) : undefined)
  let evaluatingNow = $derived(project ? isNixEvaluating(project.id) : false)
  let hasNixFiles = $derived(detection && detection.detected_files.length > 0)

  let mounted = false
  let lastProjectId: string | null = null

  onMount(() => {
    mounted = true
  })

  // Auto-detect when active project changes, and load project-specific
  // providers if a Nix env was previously evaluated
  $effect(() => {
    if (!mounted || !project) return
    if (project.id === lastProjectId) return
    lastProjectId = project.id
    void detectNix(project.id).then((det) => {
      if (det.selected?.status === 'ready') {
        void loadProvidersForProject(project!.id)
      }
    })
  })

  async function handleSelect(nixFile: string) {
    if (!project) return
    try {
      await selectNixFile(project.id, nixFile)
      notify.success('Selected Nix file', nixFile)
    } catch (error) {
      notify.error('Select Nix file failed', getErrorMessage(error))
    }
  }

  async function handleEvaluate() {
    if (!project) return
    const notificationId = notify.loading('Evaluating Nix environment')

    try {
      const record = await evaluateNix(project.id)
      await loadProvidersForProject(project.id)

      if (record.status === 'ready') {
        notify.update(notificationId, {
          title: 'Nix environment ready',
          description: record.nix_file,
          tone: 'success',
          loading: false
        })
        return
      }

      notify.update(notificationId, {
        title: record.status === 'timeout' ? 'Nix evaluation timed out' : 'Nix evaluation failed',
        description: record.error_message ?? record.nix_file,
        tone: 'error',
        loading: false
      })
    } catch (error) {
      notify.update(notificationId, {
        title: 'Nix evaluation failed',
        description: getErrorMessage(error),
        tone: 'error',
        loading: false
      })
    }
  }

  async function handleClear() {
    if (!project) return
    try {
      await clearNix(project.id)
      await loadProvidersForProject(project.id)
      notify.success('Cleared Nix environment')
    } catch (error) {
      notify.error('Clear Nix environment failed', getErrorMessage(error))
    }
  }

  function statusColor(): string {
    if (evaluatingNow) return 'text-warning'
    if (!detection?.selected) return 'text-subtle'
    switch (detection.selected.status) {
      case 'ready':
        return 'text-success'
      case 'error':
      case 'timeout':
        return 'text-danger'
      default:
        return 'text-muted'
    }
  }

  function statusLabel(): string {
    if (evaluatingNow) return 'Evaluating...'
    if (!detection?.selected) return 'Nix available'
    switch (detection.selected.status) {
      case 'ready':
        return detection.selected.nix_file
      case 'pending':
        return detection.selected.nix_file
      case 'evaluating':
        return 'Evaluating...'
      case 'error':
        return 'Error'
      case 'timeout':
        return 'Timeout'
      default:
        return detection.selected.nix_file
    }
  }
</script>

{#if hasNixFiles}
  <DropdownMenuRoot>
    <DropdownMenuTrigger
      class="flex items-center gap-1 text-xs {statusColor()} cursor-pointer border-none bg-transparent p-0 transition-colors hover:text-bright"
    >
      {#if evaluatingNow}
        <LoaderCircle size={10} class="animate-spin" />
      {:else}
        <span
          class="h-[10px] w-[10px] shrink-0 bg-current"
          style="-webkit-mask: url(/svg/nixos.svg) no-repeat center / contain; mask: url(/svg/nixos.svg) no-repeat center / contain;"
          role="img"
          aria-label="Nix"
        ></span>
      {/if}
      {statusLabel()}
      {#if detection?.selected?.status === 'ready'}
        <Check size={8} />
      {:else if detection?.selected?.status === 'error' || detection?.selected?.status === 'timeout'}
        <CircleAlert size={8} />
      {/if}
    </DropdownMenuTrigger>

    <DropdownMenuContent class="min-w-[200px]" sideOffset={6}>
      {#if detection}
        {#each detection.detected_files as file}
          {@const isSelected = detection.selected?.nix_file === file}
          <DropdownMenuItem class="flex items-center justify-between gap-2" onclick={() => handleSelect(file)}>
            <span class={isSelected ? 'text-accent' : ''}>{file}</span>
            {#if isSelected}
              <Check size={12} class="text-accent" />
            {/if}
          </DropdownMenuItem>
        {/each}

        {#if detection.selected}
          <DropdownMenuSeparator />
          {#if detection.selected.status === 'ready'}
            <DropdownMenuItem onclick={handleEvaluate}>Re-evaluate</DropdownMenuItem>
          {:else if detection.selected.status === 'error' || detection.selected.status === 'timeout'}
            <div class="px-3 py-1.5 text-sm text-danger">
              {detection.selected.error_message ?? 'Evaluation failed'}
            </div>
            <DropdownMenuItem onclick={handleEvaluate}>Retry</DropdownMenuItem>
          {:else if detection.selected.status === 'pending'}
            <DropdownMenuItem onclick={handleEvaluate}>Evaluate</DropdownMenuItem>
          {:else if detection.selected.status === 'evaluating' || evaluatingNow}
            <div class="flex items-center gap-1.5 px-3 py-1.5 text-sm text-muted">
              <LoaderCircle size={10} class="animate-spin" />
              Evaluating...
            </div>
          {/if}

          <DropdownMenuItem destructive onclick={handleClear}>
            <span class="flex items-center gap-1.5">
              <X size={12} />
              Clear Nix env
            </span>
          </DropdownMenuItem>
        {/if}
      {/if}
    </DropdownMenuContent>
  </DropdownMenuRoot>
{/if}
