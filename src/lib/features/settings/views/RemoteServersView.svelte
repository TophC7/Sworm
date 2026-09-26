<script lang="ts">
  import { onMount, tick } from 'svelte'
  import { backend } from '$lib/api/backend'
  import { requireNative } from '$lib/platform'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { copyToClipboard } from '$lib/utils/clipboard'
  import RemotePairForm from '$lib/features/remotes/RemotePairForm.svelte'
  import { pairForm } from '$lib/features/remotes/state.svelte'
  import type { RemoteSettings, RemoteStatus } from '$lib/types/backend'

  let remotes = $state<Record<string, RemoteSettings>>({})
  let statuses = $state<Record<string, RemoteStatus>>({})
  let renameServer = $state<string | null>(null)
  let name = $state('')
  let busy = $state<string | null>(null)
  let error = $state('')
  let loaded = $state(false)
  let deviceFingerprint = $state('')

  onMount(() => {
    let disposed = false
    requireNative()
      .remotes.clientFingerprint()
      .then((value) => {
        if (!disposed) deviceFingerprint = value
      })
      .catch((cause) => {
        if (!disposed) error = getErrorMessage(cause)
      })
    let generation = 0
    async function refresh() {
      const current = ++generation
      try {
        // No folder argument: remotes always belong to this desktop's global settings.
        const { settings } = await backend.settings.getEffective()
        if (disposed || current !== generation) return
        remotes = settings.remotes
        loaded = true
        await Promise.all(
          Object.keys(remotes).map(async (server) => {
            const status = await requireNative().remotes.status(server)
            if (!disposed && current === generation) statuses[server] = status
          })
        )
      } catch (cause) {
        if (!disposed) error = getErrorMessage(cause)
      }
    }
    const listeners = [
      backend.settings.onChanged(({ layer }) => {
        if (layer === 'global') void refresh()
      }),
      requireNative().remotes.onStatus(({ server, ...status }) => {
        if (!disposed) statuses[server] = status
      })
    ]
    // Fetch after subscribing so no transition lands between the two.
    void Promise.allSettled(listeners).then(() => refresh())
    return () => {
      disposed = true
      for (const listener of listeners) void listener.then((cleanup) => cleanup()).catch(() => {})
    }
  })

  async function change(server: string, action: 'rename' | 'remove') {
    if (busy) return
    busy = server
    error = ''
    try {
      if (action === 'rename') await requireNative().remotes.rename(server, name)
      else await requireNative().remotes.remove(server)
      remotes = (await backend.settings.getEffective()).settings.remotes
      renameServer = null
    } catch (cause) {
      error = getErrorMessage(cause)
    } finally {
      busy = null
    }
  }

  async function copy(value: string) {
    try {
      await copyToClipboard(value)
    } catch {
      error = 'Could not copy to the clipboard.'
    }
  }

  // Re-pair runs through the page's pairing form, targeted at this server.
  async function startRepair(server: string) {
    pairForm.input = ''
    pairForm.name = server
    await tick()
    const field = document.getElementById('remote-pair-input')
    field?.scrollIntoView({ block: 'nearest' })
    field?.focus()
  }
</script>

<section class="flex flex-col gap-3 border-b border-edge px-5 py-4">
  <div>
    <h3 class="text-md font-semibold text-bright">This Device</h3>
    <p class="text-xs text-subtle">
      Servers admit this fingerprint once paired, or when it is listed in their <span class="font-mono"
        >authorizedKeys</span
      >.
    </p>
  </div>
  <div class="flex items-start gap-2">
    <p class="min-w-0 flex-1 font-mono text-xs break-all text-muted">{deviceFingerprint || 'Loading…'}</p>
    <Button size="xs" disabled={!deviceFingerprint} onclick={() => copy(deviceFingerprint)}>Copy Fingerprint</Button>
  </div>
</section>

<RemotePairForm {remotes} />

<section class="flex flex-col gap-3 px-5 py-4">
  <div>
    <h3 class="text-md font-semibold text-bright">Saved Servers</h3>
    <p class="text-xs text-subtle">Close their tabs in every window before renaming or removing.</p>
  </div>
  {#if error}<p class="text-sm text-danger" role="alert">{error}</p>{/if}
  {#if !loaded}<p class="text-sm text-muted">Loading servers…</p>
  {:else if !Object.keys(remotes).length}<p class="text-sm text-muted">No servers paired yet.</p>{/if}
  {#each Object.entries(remotes) as [server, remote] (server)}
    {@const status = statuses[server]}
    <article
      class="flex flex-col gap-3 rounded-lg border bg-surface p-3 {pairForm.name === server
        ? 'border-accent/50'
        : 'border-edge'}"
    >
      <div class="flex items-center justify-between gap-3">
        <h4 class="text-md text-bright">{server}</h4>
        <span class="flex items-center gap-1.5 text-sm text-muted">
          <span
            class="h-2 w-2 rounded-full {status?.state === 'connected'
              ? 'bg-success'
              : status?.state === 'error'
                ? 'bg-danger'
                : status?.state === 'reconnecting'
                  ? 'bg-warning'
                  : 'bg-muted'}"
          ></span>
          {status?.state ?? 'disconnected'}
        </span>
      </div>
      <p class="font-mono text-sm break-all">{remote.address}</p>
      <div class="flex items-start gap-2">
        <p class="min-w-0 flex-1 font-mono text-xs break-all text-muted">{remote.fingerprint}</p>
        <Button size="xs" onclick={() => copy(remote.fingerprint)}>Copy Fingerprint</Button>
      </div>
      {#if status?.last_error}<p class="text-sm text-danger" role="alert">{status.last_error}</p>{/if}
      {#if renameServer === server}
        <form
          class="flex gap-2"
          onsubmit={(event) => {
            event.preventDefault()
            void change(server, 'rename')
          }}
        >
          <Input aria-label="New server name" bind:value={name} disabled={!!busy} />
          <Button
            size="sm"
            type="submit"
            disabled={!!busy || !/^[A-Za-z0-9_-]+$/.test(name) || (name !== server && name in remotes)}>Save</Button
          >
          <Button
            size="sm"
            type="button"
            disabled={!!busy}
            onclick={() => {
              renameServer = null
            }}>Cancel</Button
          >
        </form>
      {/if}
      <div class="flex flex-wrap gap-2">
        <Button
          size="xs"
          disabled={!!busy}
          onclick={() => {
            renameServer = server
            name = server
          }}>Rename</Button
        >
        <Button size="xs" variant="destructive" disabled={!!busy} onclick={() => change(server, 'remove')}
          >Remove</Button
        >
        <Button size="xs" disabled={!!busy} onclick={() => void startRepair(server)}>Re-pair</Button>
      </div>
    </article>
  {/each}
</section>
