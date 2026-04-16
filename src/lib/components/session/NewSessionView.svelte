<script lang="ts">
  import ConfirmDialog from '$lib/components/ConfirmDialog.svelte'
  import StageView from '$lib/components/StageView.svelte'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { MagicCard } from '$lib/components/ui/magic-card'
  import { InfoTooltip } from '$lib/components/ui/tooltip'
  import { allProviders, directOptions, type ProviderMeta } from '$lib/data/providers'
  import { getActiveProjectId } from '$lib/stores/projects.svelte'
  import { getConnectedProviders } from '$lib/stores/providers.svelte'
  import { createAndOpenSession, hasRunningSessions } from '$lib/stores/sessions.svelte'
  import type { ProviderStatus } from '$lib/types/backend'
  import { getErrorMessage, runNotifiedTask } from '$lib/utils/notifiedTask'

  let { onCreated }: { onCreated?: () => void } = $props()

  let connectedProviders = $derived(getConnectedProviders())
  let activeProjectId = $derived(getActiveProjectId())
  // Pre-compute Map for O(1) provider status lookups
  let providerMap = $derived(new Map(connectedProviders.map((p) => [p.id, p])))

  let sharedWarningOpen = $state(false)
  let pendingProvider = $state<ProviderStatus | null>(null)

  async function handleSelect(provider: (typeof allProviders)[number]) {
    if (!activeProjectId) return
    const status = providerMap.get(provider.id)
    if (!status) return

    if (hasRunningSessions()) {
      pendingProvider = status
      sharedWarningOpen = true
      return
    }

    await doCreateSession(status)
  }

  async function doCreateSession(provider: ProviderStatus) {
    if (!activeProjectId) return
    const created = await runNotifiedTask(
      () => createAndOpenSession(activeProjectId, provider.id, `${provider.label} session`),
      {
        loading: { title: `Starting ${provider.label} session` },
        error: {
          title: `Failed to start ${provider.label} session`,
          description: (error) => getErrorMessage(error)
        }
      }
    )
    if (created) onCreated?.()
  }
</script>

{#snippet providerCard(provider: ProviderMeta, delay: number)}
  {@const status = providerMap.get(provider.id)}
  {@const connected = status !== undefined}
  <BlurFade {delay} duration={0.4} direction="up" offset={8}>
    <MagicCard
      class="w-full rounded-xl border border-edge"
      gradientFrom={provider.gradientFrom}
      gradientTo={provider.gradientTo}
      disabled={!connected}
      onclick={() => handleSelect(provider)}
    >
      <div class="relative flex items-center gap-4 px-5 py-5">
        <img src={provider.icon} alt="" class="h-12 w-12 shrink-0 {connected ? '' : 'opacity-50 grayscale'}" />
        <div class="flex min-w-0 flex-col gap-1">
          {#if provider.textIcon && provider.textAspect}
            <span
              class="h-5 shrink-0 self-start {connected ? 'bg-fg' : 'bg-muted'}"
              style="
								width: {Math.round(20 * provider.textAspect)}px;
								-webkit-mask: url({provider.textIcon}) no-repeat center / contain;
								mask: url({provider.textIcon}) no-repeat center / contain;
							"
              role="img"
              aria-label={provider.label}
            ></span>
          {:else}
            <span
              class="shrink-0 text-[1.1rem] leading-tight font-semibold {connected ? 'text-fg' : 'text-muted'}"
              style:font-family={provider.textFont ?? 'inherit'}>{provider.textLabel ?? provider.label}</span
            >
          {/if}
          {#if !connected}
            <span class="text-[0.65rem] text-muted italic">Not detected</span>
          {:else if status?.version}
            <span class="text-[0.65rem] text-success">{status.version}</span>
          {/if}
        </div>
        {#if provider.id === 'fresh'}
          <!-- Stop click from bubbling through to MagicCard -->
          <div
            class="absolute top-1 right-1"
            role="presentation"
            onclick={(e) => e.stopPropagation()}
            onkeydown={(e) => e.stopPropagation()}
          >
            <InfoTooltip ariaLabel="What is Fresh?" contentClass="w-64">
              <p>
                A zero-config terminal text editor with VS Code-like UX. Has multi-cursor, LSP, fuzzy finder, and full
                mouse support. Not an AI agent. Give it a try!
              </p>
            </InfoTooltip>
          </div>
        {/if}
      </div>
    </MagicCard>
  </BlurFade>
{/snippet}

<StageView>
  <BlurFade delay={0.05} duration={0.5} direction="up" offset={10}>
    <h2 class="mb-1 text-center text-xl text-bright">New Session</h2>
    <p class="mb-8 text-center text-[0.82rem] text-muted">Choose a coding agent to start</p>
  </BlurFade>

  <!-- Agent CLIs -->
  <div class="grid grid-cols-2 gap-4">
    {#each allProviders as provider, i (provider.id)}
      {@render providerCard(provider, 0.1 + i * 0.08)}
    {/each}
  </div>

  <!-- Divider -->
  <BlurFade delay={0.1 + allProviders.length * 0.08} duration={0.4} direction="up" offset={8}>
    <div class="my-4 flex items-center gap-4">
      <div class="flex-1 border-t border-edge"></div>
      <span class="text-[0.75rem] text-muted">or</span>
      <div class="flex-1 border-t border-edge"></div>
    </div>
  </BlurFade>

  <!-- Direct options -->
  <div class="grid grid-cols-2 gap-4">
    {#each directOptions as provider, i (provider.id)}
      {@render providerCard(provider, 0.1 + (allProviders.length + 1 + i) * 0.08)}
    {/each}
  </div>
</StageView>

<ConfirmDialog
  open={sharedWarningOpen}
  title="Shared Workspace Warning"
  message="Another session is already running in this project.\n\nSessions in the same project share the same working tree and branch.\nChanges made by one session may conflict with another."
  confirmLabel="Start Anyway"
  onCancel={() => {
    sharedWarningOpen = false
    pendingProvider = null
  }}
  onConfirm={() => {
    sharedWarningOpen = false
    if (pendingProvider) {
      void doCreateSession(pendingProvider)
      pendingProvider = null
    }
  }}
/>
