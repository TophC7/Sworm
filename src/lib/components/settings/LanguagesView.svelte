<script lang="ts">
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { Input, Textarea } from '$lib/components/ui/input'
  import { Switch } from '$lib/components/ui/switch'
  import { CircleAlert, ChevronDown, RefreshCwIcon } from '$lib/icons/lucideExports'
  import { getActiveProjectId } from '$lib/stores/workspace.svelte'
  import { notify } from '$lib/stores/notifications.svelte'
  import { getLspServers, loadLspServers, refreshLspServers, saveLspServerConfig } from '$lib/stores/lspSettings.svelte'
  import { getErrorMessage } from '$lib/utils/notifiedTask'
  import { onDestroy } from 'svelte'
  import { createAutoSaver } from './autoSaver'

  type StatusHook = () => void
  let { onSaving, onSaved }: { onSaving: StatusHook; onSaved: StatusHook } = $props()

  let activeProjectId = $derived(getActiveProjectId())
  let servers = $derived(getLspServers())

  type Draft = {
    enabled: boolean
    binaryPath: string
    runtimePath: string
    runtimeArgs: string
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
        runtimePath: entry.config.runtime_path_override ?? '',
        runtimeArgs: entry.config.runtime_args.join(' '),
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

  async function flush(id: string) {
    const draft = drafts[id]
    if (!draft) return

    try {
      await saveLspServerConfig({
        server_definition_id: id,
        enabled: draft.enabled,
        binary_path_override: draft.binaryPath.trim() || null,
        runtime_path_override: draft.runtimePath.trim() || null,
        runtime_args: splitArgs(draft.runtimeArgs),
        extra_args: splitArgs(draft.extraArgs),
        trace: draft.trace,
        settings_json: draft.settingsJson.trim() || null
      })
      await refreshLspServers()
    } catch (error) {
      notify.error('Save language server failed', getErrorMessage(error))
    }
  }

  function update(id: string, key: keyof Draft, value: boolean | string) {
    drafts = { ...drafts, [id]: { ...drafts[id], [key]: value } }
    saver.schedule(id, () => flush(id))
  }

  function toggleExpanded(id: string) {
    expanded = { ...expanded, [id]: !(expanded[id] ?? false) }
  }

  async function refresh() {
    try {
      await refreshLspServers()
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
</script>

<section class="flex items-start justify-between gap-4 px-5 py-4">
  <div class="flex flex-col gap-1">
    <h3 class="text-md font-semibold text-bright">Language servers</h3>
    <p class="max-w-2xl text-xs text-subtle">
      Sworm loads a curated LSP catalog and lets you override the runtime where it matters. Executables stay
      app-level only; repositories do not get to decide what Sworm spawns. Status resolves against the active
      project's environment when a project is open.
    </p>
  </div>

  <Button variant="outline" size="xs" onclick={refresh}>
    <RefreshCwIcon size={12} />
    Refresh
  </Button>
</section>

<div class="flex flex-col border-t border-edge">
  {#each servers as entry (entry.server.server_definition_id)}
    {@const id = entry.server.server_definition_id}
    {@const draft = drafts[id]}
    {@const open = expanded[id] ?? false}

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
        <div role="presentation" onclick={(event) => event.stopPropagation()} onkeydown={(event) => event.stopPropagation()}>
          <Switch checked={draft?.enabled ?? true} onCheckedChange={(value) => update(id, 'enabled', value)} />
        </div>

        <div class="flex min-w-0 flex-1 flex-col gap-0.5">
          <div class="flex items-center gap-2">
            <span class="text-sm font-semibold text-bright">{entry.server.label}</span>
            <Badge variant={statusVariant(entry.server.status)}>{entry.server.status}</Badge>
            <span class="text-xs text-subtle">{entry.server.extension_label}</span>
          </div>
          <span class="truncate font-mono text-xs text-muted">
            {entry.server.runtime_resolved_path
              ? `${entry.server.runtime_resolved_path} -> ${entry.server.resolved_path ?? entry.server.install_hint}`
              : entry.server.resolved_path ?? entry.server.install_hint}
          </span>
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
          <div class="flex items-center gap-3">
            <span class="w-32 shrink-0 text-sm text-muted">Binary path</span>
            <Input
              class="flex-1 py-2"
              value={draft.binaryPath}
              oninput={(event: Event) => update(id, 'binaryPath', (event.currentTarget as HTMLInputElement).value)}
              placeholder="Use detected binary"
            />
          </div>

          <div class="flex items-center gap-3">
            <span class="w-32 shrink-0 text-sm text-muted">Runtime path</span>
            <Input
              class="flex-1 py-2"
              value={draft.runtimePath}
              oninput={(event: Event) => update(id, 'runtimePath', (event.currentTarget as HTMLInputElement).value)}
              placeholder="Only needed for JS-backed servers"
            />
          </div>

          <div class="flex items-center gap-3">
            <span class="w-32 shrink-0 text-sm text-muted">Runtime args</span>
            <Input
              class="flex-1 py-2"
              value={draft.runtimeArgs}
              oninput={(event: Event) => update(id, 'runtimeArgs', (event.currentTarget as HTMLInputElement).value)}
              placeholder="--flag value"
            />
          </div>

          <div class="flex items-center gap-3">
            <span class="w-32 shrink-0 text-sm text-muted">Extra args</span>
            <Input
              class="flex-1 py-2"
              value={draft.extraArgs}
              oninput={(event: Event) => update(id, 'extraArgs', (event.currentTarget as HTMLInputElement).value)}
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
                  onclick={() => update(id, 'trace', trace)}
                >
                  {trace}
                </Button>
              {/each}
            </div>
          </div>

          <div class="flex items-start gap-3">
            <span class="w-32 shrink-0 pt-2 text-sm text-muted">Settings JSON</span>
            <Textarea
              class="min-h-24 flex-1 font-mono text-xs"
              value={draft.settingsJson}
              oninput={(event: Event) => update(id, 'settingsJson', (event.currentTarget as HTMLTextAreaElement).value)}
              placeholder="JSON object for workspace/configuration"
            />
          </div>
        </div>
      {/if}
    </div>
  {/each}
</div>
