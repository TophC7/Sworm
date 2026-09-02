<script lang="ts">
  import StageView from '$lib/components/layout/StageView.svelte'
  import { BlurFade } from '$lib/components/ui/blur-fade'
  import { MagicCard } from '$lib/components/ui/magic-card'
  import { Separator } from '$lib/components/ui/separator'
  import { allProviders, directOptions, type ProviderMeta } from '$lib/features/sessions/providers/catalog'
  import { getConnectedProviders, getProvidersLoading } from '$lib/features/sessions/providers/state.svelte'
  import { startSession } from '$lib/features/sessions/service.svelte'
  import { createSession } from '$lib/features/app-actions/actions.svelte'

  let { folderPath }: { folderPath: string } = $props()

  let providersLoading = $derived(getProvidersLoading())
  // Pre-compute Map for O(1) provider status lookups
  let providerMap = $derived(new Map(getConnectedProviders(folderPath).map((p) => [p.id, p])))

  function handleSelect(provider: ProviderMeta) {
    if (!providerMap.has(provider.id)) return
    if (provider.id === 'terminal') {
      startSession(folderPath, 'terminal', 'Terminal')
      return
    }
    createSession(provider.id, provider.label)
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
              class="shrink-0 text-2xl leading-tight font-semibold {connected ? 'text-fg' : 'text-muted'}"
              style:font-family={provider.textFont ?? 'inherit'}>{provider.textLabel ?? provider.label}</span
            >
          {/if}
          {#if !connected}
            <span class="text-2xs text-muted italic">{providersLoading ? 'Detecting...' : 'Not detected'}</span>
          {:else if status?.version}
            <span class="text-2xs text-success">{status.version}</span>
          {/if}
        </div>
      </div>
    </MagicCard>
  </BlurFade>
{/snippet}

<StageView>
  <BlurFade delay={0.05} duration={0.5} direction="up" offset={10}>
    <h2 class="mb-1 text-center text-xl text-bright">New Session</h2>
    <p class="mb-8 text-center text-base text-muted">Choose a coding agent to start</p>
  </BlurFade>

  <!-- Agent CLIs -->
  <div class="grid grid-cols-2 gap-4">
    {#each allProviders as provider, i (provider.id)}
      {@render providerCard(provider, 0.1 + i * 0.08)}
    {/each}
  </div>

  <!-- Divider between agent and direct options -->
  <BlurFade delay={0.1 + allProviders.length * 0.08} duration={0.4} direction="up" offset={8}>
    <div class="my-4 flex items-center gap-4">
      <Separator class="flex-1" />
      <span class="text-sm text-muted">or</span>
      <Separator class="flex-1" />
    </div>
  </BlurFade>

  <!-- Direct options -->
  <div class="grid grid-cols-2 gap-4">
    {#each directOptions as provider, i (provider.id)}
      {@render providerCard(provider, 0.1 + (allProviders.length + 1 + i) * 0.08)}
    {/each}
  </div>
</StageView>
