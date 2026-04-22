<script lang="ts">
  import { getBuiltinLanguageLabel } from '$lib/features/builtins/catalog'
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { Switch } from '$lib/components/ui/switch'
  import { TabsList, TabsRoot, TabsTrigger } from '$lib/components/ui/tabs'
  import { Braces, CircleAlert, ChevronDown, RefreshCwIcon } from '$lib/icons/lucideExports'
  import {
    getLspServers,
    getLspServersLoading,
    loadLspServers,
    refreshLspServers,
    saveLspServerConfig
  } from '$lib/features/settings/state/lspSettings.svelte'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getSettings, saveFormattingSettings } from '$lib/features/settings/state/settings.svelte'
  import { getActiveProjectId } from '$lib/features/workbench/state.svelte'
  import type { BuiltinSettingsPage, FormatterSelection, LspServerSettingsEntry } from '$lib/types/backend'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { onDestroy } from 'svelte'
  import { createAutoSaver } from './autoSaver'
  import type { JsonSettingsEditorSession } from './jsonSettings'
  import {
    serializeSettingsEditorValue,
    settingsEditorDefaults,
    settingsEditorDescription,
    settingsEditorSchema,
    settingsEditorValue
  } from './jsonSettings'

  type StatusHook = () => void

  let {
    definition,
    onOpenJsonEditor,
    onSaving,
    onSaved
  }: {
    definition: BuiltinSettingsPage
    onOpenJsonEditor: (session: JsonSettingsEditorSession) => void
    onSaving: StatusHook
    onSaved: StatusHook
  } = $props()

  let settings = $derived(getSettings())
  let activeProjectId = $derived(getActiveProjectId())
  let lspLoading = $derived(getLspServersLoading())
  let allServers = $derived(getLspServers())
  let servers = $derived(
    allServers.filter((entry) => definition.server_definition_ids.includes(entry.server.server_definition_id))
  )
  let selectedFormatter = $derived(
    definition.formatter && settings ? settings.formatting[definition.formatter.group].formatter : null
  )

  type Draft = {
    enabled: boolean
    binaryPath: string
    extraArgs: string
    trace: 'off' | 'messages' | 'verbose'
    settingsJson: string
  }

  let drafts = $state<Record<string, Draft>>({})
  let expanded = $state<Record<string, boolean>>({})
  const seeded = new Set<string>()

  $effect(() => {
    for (const entry of servers) {
      const id = entry.server.server_definition_id
      if (seeded.has(id)) continue
      seeded.add(id)
      drafts[id] = {
        enabled: entry.config.enabled,
        binaryPath: entry.config.binary_path_override ?? '',
        extraArgs: entry.config.extra_args.join(' '),
        trace: entry.config.trace,
        settingsJson: entry.config.settings_json ?? ''
      }
    }
  })

  const saver = createAutoSaver({
    onBusyChange: (busy) => (busy ? onSaving() : onSaved())
  })

  let lastLoadedProjectId = $state<string | undefined>(undefined)
  $effect(() => {
    const projectId = activeProjectId ?? undefined
    if (lastLoadedProjectId === projectId) return
    lastLoadedProjectId = projectId
    void loadLspServers(projectId)
  })

  onDestroy(() => saver.dispose())

  async function saveFormatter(formatter: FormatterSelection) {
    if (!definition.formatter || !settings || selectedFormatter === formatter) return
    onSaving()
    try {
      await saveFormattingSettings({
        ...settings.formatting,
        [definition.formatter.group]: {
          ...settings.formatting[definition.formatter.group],
          formatter
        }
      })
    } catch (error) {
      notify.error('Save formatting settings failed', getErrorMessage(error))
    } finally {
      onSaved()
    }
  }

  async function flushServer(id: string) {
    const draft = drafts[id]
    if (!draft) return

    await persistServer(id, draft)
  }

  async function persistServer(id: string, draft: Draft) {
    await saveLspServerConfig(
      {
        server_definition_id: id,
        enabled: draft.enabled,
        binary_path_override: draft.binaryPath.trim() || null,
        runtime_path_override: null,
        runtime_args: [],
        extra_args: splitArgs(draft.extraArgs),
        trace: draft.trace,
        settings_json: draft.settingsJson.trim() || null
      },
      activeProjectId ?? undefined
    )
  }

  async function flushServerQuietly(id: string) {
    try {
      await flushServer(id)
    } catch (error) {
      notify.error('Save language server failed', getErrorMessage(error))
    }
  }

  function updateServer(id: string, key: keyof Draft, value: boolean | string) {
    const draft = drafts[id]
    if (!draft) return
    drafts = { ...drafts, [id]: { ...draft, [key]: value } }
    saver.schedule(id, () => flushServerQuietly(id))
  }

  function toggleExpanded(id: string) {
    expanded = { ...expanded, [id]: !(expanded[id] ?? false) }
  }

  async function handleJsonSettingsSave(id: string, nextValue: string | null) {
    const draft = drafts[id]
    if (!draft) return
    const nextDraft = { ...draft, settingsJson: nextValue ?? '' }
    drafts = { ...drafts, [id]: nextDraft }
    onSaving()
    try {
      await persistServer(id, nextDraft)
    } catch (error) {
      notify.error('Save language server failed', getErrorMessage(error))
      throw error
    } finally {
      onSaved()
    }
  }

  async function refresh() {
    try {
      await refreshLspServers(activeProjectId ?? undefined)
    } catch (error) {
      notify.error('Refresh language servers failed', getErrorMessage(error))
    }
  }

  function statusVariant(status: string): 'success' | 'warning' | 'danger' | 'default' {
    if (status === 'connected') return 'success'
    if (status === 'missing') return 'warning'
    if (status === 'error') return 'danger'
    return 'default'
  }

  function splitArgs(value: string): string[] {
    return value
      .split(/\s+/)
      .map((segment) => segment.trim())
      .filter(Boolean)
  }

  function relatedLanguages(entry: LspServerSettingsEntry): string[] {
    const pageLanguages = new Set(definition.language_ids)
    const languages = new Set<string>()
    for (const selector of entry.server.document_selectors) {
      if (selector.language && !pageLanguages.has(selector.language)) {
        languages.add(selector.language)
      }
    }
    return [...languages].sort()
  }

  function sharedConfigScope(extraLanguages: string[]): string | null {
    if (extraLanguages.length === 0) return null
    return `${definition.label} and ${extraLanguages.map(getBuiltinLanguageLabel).join(', ')}`
  }

  function openJsonEditor(entry: LspServerSettingsEntry, draft: Draft, extraLanguages: string[]) {
    const settings = entry.server.settings
    const sharedScope = sharedConfigScope(extraLanguages)
    onOpenJsonEditor({
      id: entry.server.server_definition_id,
      title: entry.server.label,
      description: settingsEditorDescription(settings, sharedScope),
      schema: settingsEditorSchema(settings),
      defaults: settingsEditorDefaults(settings),
      value: settingsEditorValue(settings, draft.settingsJson),
      onSave: async (nextValue) => {
        await handleJsonSettingsSave(
          entry.server.server_definition_id,
          serializeSettingsEditorValue(settings, nextValue)
        )
      }
    })
  }

  function formatterLabel(option: FormatterSelection): string {
    switch (option) {
      case 'biome':
        return 'Biome'
      case 'nixfmt':
        return 'nixfmt'
      case 'disabled':
        return 'Disabled'
      case 'lsp':
      default:
        return 'LSP'
    }
  }
</script>

<section class="border-b border-edge px-5 py-4">
  <div class="flex flex-col gap-2">
    <h4 class="text-sm font-semibold text-bright">Formatting</h4>

    {#if definition.formatter}
      {#if settings}
        <TabsRoot
          value={selectedFormatter ?? definition.formatter.default}
          onValueChange={(value) => saveFormatter(value as FormatterSelection)}
        >
          <TabsList>
            {#each definition.formatter.options as option (option)}
              <TabsTrigger value={option}>{formatterLabel(option)}</TabsTrigger>
            {/each}
          </TabsList>
        </TabsRoot>
      {:else}
        <p class="text-xs text-subtle">Loading formatting settings&hellip;</p>
      {/if}
    {:else}
      <p class="text-xs text-subtle">Formatting currently follows the language server path.</p>
      <div>
        <Badge variant="muted">LSP</Badge>
      </div>
    {/if}
  </div>
</section>

<section class="flex items-start justify-between gap-4 px-5 py-4">
  <h4 class="text-sm font-semibold text-bright">Language servers</h4>

  <Button variant="outline" size="xs" onclick={refresh}>
    <RefreshCwIcon size={12} />
    Refresh
  </Button>
</section>

{#if lspLoading && servers.length === 0}
  <div class="border-t border-edge px-5 py-8 text-sm text-muted">Loading language servers&hellip;</div>
{:else if servers.length === 0}
  <div class="border-t border-edge px-5 py-8 text-sm text-muted">
    No language servers are attached to this language yet.
  </div>
{:else}
  <div class="flex flex-col border-t border-edge">
    {#each servers as entry (entry.server.server_definition_id)}
      {@const id = entry.server.server_definition_id}
      {@const draft = drafts[id]}
      {@const open = expanded[id] ?? false}
      {@const extraLanguages = relatedLanguages(entry)}

      <div class="border-b border-edge/70">
        <div
          role="button"
          tabindex="0"
          onclick={() => toggleExpanded(id)}
          onkeydown={(event) => {
            if (event.key === 'Enter' || event.key === ' ') {
              event.preventDefault()
              toggleExpanded(id)
            }
          }}
          class="group flex cursor-pointer items-center gap-3 px-5 py-3 transition-colors hover:bg-surface/40"
        >
          <div
            role="presentation"
            onclick={(event) => event.stopPropagation()}
            onkeydown={(event) => event.stopPropagation()}
          >
            <Switch checked={draft?.enabled ?? true} onCheckedChange={(value) => updateServer(id, 'enabled', value)} />
          </div>

          <div class="flex min-w-0 flex-1 flex-col gap-0.5">
            <div class="flex items-center gap-2">
              <span class="text-sm font-semibold text-bright">{entry.server.label}</span>
              <Badge variant={statusVariant(entry.server.status)}>{entry.server.status}</Badge>
            </div>
            <span class="truncate font-mono text-xs text-muted">
              {entry.server.resolved_path ?? entry.server.install_hint}
            </span>
            {#if extraLanguages.length > 0}
              <span class="text-xs text-subtle">Shared config</span>
            {/if}
          </div>

          <ChevronDown
            size={14}
            class="shrink-0 text-subtle transition-transform group-hover:text-fg {open ? 'rotate-180' : ''}"
          />
        </div>

        {#if entry.server.message}
          <div class="flex items-center gap-2 bg-danger-bg/40 px-5 py-2 text-sm text-danger-bright">
            <CircleAlert size={14} />
            <span>{entry.server.message}</span>
          </div>
        {/if}

        {#if open && draft}
          <div class="flex flex-col gap-3 bg-surface/40 px-5 py-4">
            {#if entry.server.runtime_resolved_path}
              <div class="flex items-center gap-3">
                <span class="w-32 shrink-0 text-sm text-muted">Runtime</span>
                <span class="truncate font-mono text-xs text-muted">{entry.server.runtime_resolved_path}</span>
              </div>
            {/if}

            <div class="flex items-center gap-3">
              <span class="w-32 shrink-0 text-sm text-muted">Binary path</span>
              <Input
                class="flex-1 py-2"
                value={draft.binaryPath}
                oninput={(event: Event) =>
                  updateServer(id, 'binaryPath', (event.currentTarget as HTMLInputElement).value)}
                placeholder="Use detected binary"
              />
            </div>

            <div class="flex items-center gap-3">
              <span class="w-32 shrink-0 text-sm text-muted">Extra args</span>
              <Input
                class="flex-1 py-2"
                value={draft.extraArgs}
                oninput={(event: Event) =>
                  updateServer(id, 'extraArgs', (event.currentTarget as HTMLInputElement).value)}
                placeholder="--stdio already comes from the manifest"
              />
            </div>

            <div class="flex items-start gap-3">
              <span class="w-32 shrink-0 pt-1 text-sm text-muted">Trace</span>
              <div class="flex flex-wrap gap-2">
                {#each ['off', 'messages', 'verbose'] as trace}
                  <Button
                    variant={draft.trace === trace ? 'outline' : 'ghost'}
                    size="xs"
                    onclick={() => updateServer(id, 'trace', trace)}
                  >
                    {trace}
                  </Button>
                {/each}
              </div>
            </div>

            <div class="flex items-start gap-3">
              <span class="w-32 shrink-0 pt-1.5 text-sm text-muted">Preferences</span>
              <div class="flex flex-1 flex-col gap-1.5">
                <div class="flex items-center gap-2">
                  <Button variant="outline" size="xs" onclick={() => openJsonEditor(entry, draft, extraLanguages)}>
                    <Braces size={12} />
                    {draft.settingsJson.trim() ? 'Edit JSON' : 'Set JSON'}
                  </Button>
                  {#if draft.settingsJson.trim()}
                    <Button
                      variant="ghost"
                      size="xs"
                      onclick={() => {
                        drafts = { ...drafts, [id]: { ...draft, settingsJson: '' } }
                        void flushServer(id)
                      }}
                    >
                      Clear
                    </Button>
                  {/if}
                </div>
              </div>
            </div>
          </div>
        {/if}
      </div>
    {/each}
  </div>
{/if}
