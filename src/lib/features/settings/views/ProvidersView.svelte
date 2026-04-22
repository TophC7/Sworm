<!--
  @component
  ProvidersView — coding-agent CLI roster.

  Flat, IDE-style row list. Each row: switch + name/status/version
  + resolved path + chevron toggle. Expanding reveals inline rows
  for binary-path override and extra-args. Writes debounce through
  a shared autoSaver so fast typing collapses into one flush.

  @param onSaving - flagged when the autoSaver goes busy
  @param onSaved - flagged when it goes idle
-->

<script lang="ts">
  import { CircleAlert, ChevronDown } from '$lib/icons/lucideExports'
  import { Badge } from '$lib/components/ui/badge'
  import { Input } from '$lib/components/ui/input'
  import { Switch } from '$lib/components/ui/switch'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { refreshProviders } from '$lib/features/sessions/providers/state.svelte'
  import { getSettings, saveProviderConfig } from '$lib/features/settings/state/settings.svelte'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { onDestroy } from 'svelte'
  import { createAutoSaver } from './autoSaver'

  type StatusHook = () => void
  let { onSaving, onSaved }: { onSaving: StatusHook; onSaved: StatusHook } = $props()

  let settings = $derived(getSettings())

  type Draft = { enabled: boolean; binaryPath: string; extraArgs: string }
  let drafts = $state<Record<string, Draft>>({})
  let expanded = $state<Record<string, boolean>>({})

  // Seed drafts once per provider id. Subsequent backend refreshes
  // must not clobber in-progress edits, so we only populate ids we
  // haven't seen before.
  const seeded = new Set<string>()
  $effect(() => {
    if (!settings) return
    for (const entry of settings.providers) {
      const id = entry.provider.id
      if (seeded.has(id)) continue
      seeded.add(id)
      drafts[id] = {
        enabled: entry.config.enabled,
        binaryPath: entry.config.binary_path_override ?? '',
        extraArgs: entry.config.extra_args.join(' ')
      }
    }
  })

  const saver = createAutoSaver({
    onBusyChange: (busy) => (busy ? onSaving() : onSaved())
  })

  onDestroy(() => saver.dispose())

  async function flush(id: string) {
    const d = drafts[id]
    if (!d) return
    try {
      await saveProviderConfig({
        provider_id: id,
        enabled: d.enabled,
        binary_path_override: d.binaryPath.trim() || null,
        extra_args: d.extraArgs
          .split(/\s+/)
          .map((v) => v.trim())
          .filter(Boolean)
      })
      // NOTE: a binary_path_override change can flip resolved detection;
      // refreshProviders picks up the new status/version. The settings
      // store itself was already patched by saveProviderConfig, so no
      // full settings refetch is needed here.
      await refreshProviders()
    } catch (error) {
      notify.error('Save provider failed', getErrorMessage(error))
    }
  }

  function update(id: string, key: keyof Draft, value: boolean | string) {
    drafts = { ...drafts, [id]: { ...drafts[id], [key]: value } }
    saver.schedule(id, () => flush(id))
  }

  function toggleExpanded(id: string) {
    expanded = { ...expanded, [id]: !(expanded[id] ?? false) }
  }

  function statusVariant(s: string): 'success' | 'warning' | 'danger' | 'default' {
    if (s === 'connected') return 'success'
    if (s === 'missing') return 'warning'
    if (s === 'error') return 'danger'
    return 'default'
  }
</script>

<section class="flex flex-col gap-1 px-5 py-4">
  <h3 class="text-md font-semibold text-bright">Coding agent CLIs</h3>
  <p class="pb-2 text-xs text-subtle">Enabled providers appear in New Session.</p>
</section>

{#if settings}
  <div class="flex flex-col">
    {#each settings.providers as entry (entry.provider.id)}
      {@const id = entry.provider.id}
      {@const d = drafts[id]}
      {@const open = expanded[id] ?? false}

      <div class="border-t border-edge">
        <div
          role="button"
          tabindex="0"
          onclick={() => toggleExpanded(id)}
          onkeydown={(e) => {
            if (e.key === 'Enter' || e.key === ' ') {
              e.preventDefault()
              toggleExpanded(id)
            }
          }}
          class="group flex cursor-pointer items-center gap-3 px-5 py-3 transition-colors hover:bg-surface/40"
        >
          <!-- Switch owns its own click; stop propagation so toggling the
               provider doesn't also expand the row. -->
          <div role="presentation" onclick={(e) => e.stopPropagation()} onkeydown={(e) => e.stopPropagation()}>
            <Switch checked={d?.enabled ?? true} onCheckedChange={(v) => update(id, 'enabled', v)} />
          </div>

          <div class="flex min-w-0 flex-1 flex-col gap-0.5">
            <div class="flex items-center gap-2">
              <span class="text-sm font-semibold text-bright">{entry.provider.label}</span>
              <Badge variant={statusVariant(entry.provider.status)}>
                {entry.provider.status}
              </Badge>
              {#if entry.provider.version}
                <span class="font-mono text-xs text-subtle">{entry.provider.version}</span>
              {/if}
            </div>
            <span class="truncate font-mono text-xs text-muted">
              {entry.provider.resolved_path ?? entry.provider.install_hint}
            </span>
          </div>

          <ChevronDown
            size={14}
            class="shrink-0 text-subtle transition-transform group-hover:text-fg {open ? 'rotate-180' : ''}"
          />
        </div>

        {#if entry.provider.message}
          <div
            class="flex items-center gap-2 border-t border-danger/30 bg-danger-bg px-5 py-2 text-sm text-danger-bright"
          >
            <CircleAlert size={14} />
            <span>{entry.provider.message}</span>
          </div>
        {/if}

        {#if open && d}
          <div class="flex flex-col gap-3 border-t border-edge bg-surface/40 px-5 py-4">
            <div class="flex items-center">
              <span class="w-32 shrink-0 text-sm text-muted">Binary path</span>
              <Input
                class="flex-1 py-2"
                value={d.binaryPath}
                oninput={(e: Event) => update(id, 'binaryPath', (e.currentTarget as HTMLInputElement).value)}
                placeholder="Use detected CLI"
              />
            </div>

            <div class="flex items-center">
              <span class="w-32 shrink-0 text-sm text-muted">Extra args</span>
              <Input
                class="flex-1 py-2"
                value={d.extraArgs}
                oninput={(e: Event) => update(id, 'extraArgs', (e.currentTarget as HTMLInputElement).value)}
                placeholder="--flag value"
              />
            </div>
          </div>
        {/if}
      </div>
    {/each}
  </div>
{:else}
  <div class="px-5 py-8 text-sm text-muted">Loading providers&hellip;</div>
{/if}
