<script lang="ts">
  import { Button } from '$lib/components/ui/button'
  import { Badge } from '$lib/components/ui/badge'
  import { getSettings, saveFormattingSettings } from '$lib/stores/settings.svelte'
  import { notify } from '$lib/stores/notifications.svelte'
  import type { FormattingSettings, FormatterSelection } from '$lib/types/backend'
  import { getErrorMessage } from '$lib/utils/notifiedTask'
  import { onDestroy } from 'svelte'
  import { createAutoSaver } from './autoSaver'

  type StatusHook = () => void
  let { onSaving, onSaved }: { onSaving: StatusHook; onSaved: StatusHook } = $props()

  let settings = $derived(getSettings())
  let draft = $state<FormattingSettings | null>(null)
  let seeded = $state(false)

  const GROUPS: Array<{
    id: keyof FormattingSettings
    label: string
    description: string
    languages: string[]
    autoLabel: string
    options: Array<{ value: FormatterSelection; label: string }>
  }> = [
    {
      id: 'javascript_typescript',
      label: 'JavaScript / TypeScript',
      description: 'Keep TS intelligence on vtsls while choosing who formats JS, TS, JSX, and TSX files.',
      languages: ['js', 'jsx', 'ts', 'tsx'],
      autoLabel: 'Auto → Biome',
      options: [
        { value: 'auto', label: 'Auto' },
        { value: 'biome', label: 'Biome' },
        { value: 'lsp', label: 'LSP' },
        { value: 'disabled', label: 'Disabled' }
      ]
    },
    {
      id: 'json',
      label: 'JSON',
      description: 'Use Biome for JSON and JSONC formatting without depending on a separate JSON formatter flow.',
      languages: ['json', 'jsonc'],
      autoLabel: 'Auto → Biome',
      options: [
        { value: 'auto', label: 'Auto' },
        { value: 'biome', label: 'Biome' },
        { value: 'lsp', label: 'LSP' },
        { value: 'disabled', label: 'Disabled' }
      ]
    },
    {
      id: 'nix',
      label: 'Nix',
      description: 'Prefer direct nixfmt formatting by default while still allowing nil to own formatting later if you want it.',
      languages: ['nix'],
      autoLabel: 'Auto → nixfmt',
      options: [
        { value: 'auto', label: 'Auto' },
        { value: 'nixfmt', label: 'nixfmt' },
        { value: 'lsp', label: 'LSP' },
        { value: 'disabled', label: 'Disabled' }
      ]
    }
  ]

  $effect(() => {
    if (seeded || !settings) return
    draft = cloneFormattingSettings(settings.formatting)
    seeded = true
  })

  const saver = createAutoSaver({
    onBusyChange: (busy) => (busy ? onSaving() : onSaved())
  })

  onDestroy(() => saver.dispose())

  async function flush() {
    if (!draft) return
    try {
      await saveFormattingSettings(draft)
    } catch (error) {
      notify.error('Save formatting settings failed', getErrorMessage(error))
    }
  }

  function update(group: keyof FormattingSettings, formatter: FormatterSelection) {
    if (!draft) return
    draft = {
      ...draft,
      [group]: {
        ...draft[group],
        formatter
      }
    }
    saver.schedule(group, () => flush())
  }

  function cloneFormattingSettings(value: FormattingSettings): FormattingSettings {
    return {
      javascript_typescript: { ...value.javascript_typescript },
      json: { ...value.json },
      nix: { ...value.nix }
    }
  }
</script>

<section class="flex flex-col gap-1 px-5 py-4">
  <h3 class="text-md font-semibold text-bright">Formatting</h3>
  <p class="max-w-2xl text-xs text-subtle">
    Formatting is separate from language intelligence. Pick the formatter owner for each language group without
    changing which language server provides hover, completion, or definitions.
  </p>
</section>

{#if draft}
  <div class="flex flex-col border-t border-edge">
    {#each GROUPS as group (group.id)}
      {@const selected = draft[group.id].formatter}
      <section class="border-b border-edge/70 px-5 py-4">
        <div class="flex flex-col gap-3">
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="flex min-w-0 flex-1 flex-col gap-1">
              <div class="flex flex-wrap items-center gap-2">
                <h4 class="text-sm font-semibold text-bright">{group.label}</h4>
                <span class="text-xs text-subtle">{group.autoLabel}</span>
              </div>
              <p class="max-w-2xl text-xs text-subtle">{group.description}</p>
            </div>

            <div class="flex flex-wrap gap-1.5">
              {#each group.languages as language (language)}
                <Badge variant="default">{language}</Badge>
              {/each}
            </div>
          </div>

          <div class="flex flex-wrap gap-2">
            {#each group.options as option (option.value)}
              <Button
                variant={selected === option.value ? 'outline' : 'ghost'}
                size="xs"
                onclick={() => update(group.id, option.value)}
              >
                {option.label}
              </Button>
            {/each}
          </div>
        </div>
      </section>
    {/each}
  </div>
{:else}
  <div class="px-5 py-8 text-sm text-muted">Loading formatting settings&hellip;</div>
{/if}
