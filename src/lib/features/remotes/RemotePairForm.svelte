<!--
  @component
  RemotePairForm — pairing on the Remote Servers settings page. One field
  takes either a host (answers with the `sworm-server pair` command to run)
  or a `sworm-pair://` link (previews it, suggests a name, pairs). A link
  for a saved server, or a name that is already saved, re-pairs it.
-->

<script lang="ts">
  import { backend } from '$lib/api/backend'
  import { Alert } from '$lib/components/ui/alert'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { notify } from '$lib/features/notifications/state.svelte'
  import type { RemoteSettings } from '$lib/types/backend'
  import { copyToClipboard } from '$lib/utils/clipboard'
  import { pairCommand, pairForm, parsePairLink } from './state.svelte'

  let { remotes }: { remotes: Record<string, RemoteSettings> } = $props()

  let busy = $state(false)
  let error = $state('')

  const input = $derived(pairForm.input.trim())
  const link = $derived(parsePairLink(input))
  const typingLink = $derived(input.startsWith('sworm-pair:'))
  const linkAddress = $derived(link ? `${link.host.includes(':') ? `[${link.host}]` : link.host}:${link.port}` : null)
  const fingerprintKey = (fingerprint: string) => fingerprint.replace(/^SHA256:/i, '').toLowerCase()

  // The saved server this link belongs to: same identity first, then same address.
  const match = $derived.by(() => {
    if (!link) return null
    const entries = Object.entries(remotes)
    const found =
      entries.find(([, remote]) => fingerprintKey(remote.fingerprint) === fingerprintKey(link.fingerprint)) ??
      entries.find(([, remote]) => remote.address === linkAddress)
    return found?.[0] ?? null
  })

  // A suggestion never lands on an unrelated saved name: that would silently
  // replace its pairing.
  const suggestedName = $derived.by(() => {
    if (match) return match
    const base = link?.name ?? ''
    if (!(base in remotes)) return base
    let suffix = 2
    while (`${base}-${suffix}` in remotes) suffix++
    return `${base}-${suffix}`
  })

  const name = $derived(pairForm.name ?? suggestedName)
  const validName = $derived(/^[A-Za-z0-9_-]+$/.test(name))
  const existing = $derived(Object.hasOwn(remotes, name) ? remotes[name] : null)
  // Re-pair picked from a server card, waiting for its link.
  const repairTarget = $derived(pairForm.name !== null && existing ? pairForm.name : null)
  const identityChanged = $derived(
    !!existing && !!link && fingerprintKey(existing.fingerprint) !== fingerprintKey(link.fingerprint)
  )
  const addressChanged = $derived(!!existing && !!linkAddress && existing.address !== linkAddress)

  // Where a link comes from: the typed host, or the server being re-paired.
  const command = $derived.by(() => {
    if (!input) return existing ? pairCommand(existing.address) : null
    return typingLink || input.includes('://') ? null : pairCommand(input)
  })

  function reset(): void {
    pairForm.input = ''
    pairForm.name = null
    error = ''
  }

  async function copy(value: string): Promise<void> {
    try {
      await copyToClipboard(value)
    } catch {
      error = 'Could not copy to the clipboard.'
    }
  }

  async function pair(): Promise<void> {
    if (busy || !link || !validName) return
    busy = true
    error = ''
    const replacing = !!existing
    try {
      await (replacing ? backend.remotes.repair(input, name) : backend.remotes.pair(input, name))
      notify.success(replacing ? `Re-paired ${name}` : `Paired ${name}`)
      reset()
    } catch {
      // Never surface backend text here: authentication errors may contain the submitted secret.
      error =
        'Pairing failed. The link may have expired or the server is unreachable. Request a fresh link and try again.'
    } finally {
      busy = false
    }
  }
</script>

<section class="flex flex-col gap-3 border-b border-edge px-5 py-4">
  <div class="flex items-start gap-3">
    <div class="min-w-0 flex-1">
      <h3 class="text-md font-semibold text-bright">{repairTarget ? `Re-pair ${repairTarget}` : 'Add Server'}</h3>
      <p class="text-xs text-subtle">
        Type the server's host to get the command that prints a pairing link, then paste the link here.
      </p>
    </div>
    {#if pairForm.input || pairForm.name !== null}
      <Button size="xs" variant="ghost" disabled={busy} onclick={reset}>Clear</Button>
    {/if}
  </div>

  <form
    class="flex flex-col gap-3"
    onsubmit={(event) => {
      event.preventDefault()
      void pair()
    }}
  >
    <!-- Masked once it is a link: the token in it is a secret. -->
    <Input
      id="remote-pair-input"
      type={typingLink ? 'password' : 'text'}
      bind:value={pairForm.input}
      placeholder="homelab.local  or  sworm-pair://…"
      aria-label="Server host or pairing link"
      autocomplete="off"
      spellcheck={false}
      disabled={busy}
      class="font-mono text-sm"
      oninput={() => (error = '')}
    />

    {#if command}
      <div class="flex items-center gap-2 rounded-lg border border-edge bg-surface py-1.5 pr-1.5 pl-3">
        <code class="min-w-0 flex-1 truncate font-mono text-xs text-fg" title={command}>{command}</code>
        <Button size="xs" type="button" onclick={() => void copy(command)}>Copy</Button>
      </div>
      <p class="text-xs text-subtle">
        Run it, then paste the sworm-pair:// link it prints. Links expire after 10 minutes.
      </p>
    {:else if typingLink && !link}
      <p class="text-sm text-danger" role="alert">Incomplete link. Copy the whole line starting with sworm-pair://.</p>
    {/if}

    {#if link}
      <div class="flex flex-col gap-3 rounded-lg border border-edge bg-surface p-3">
        <dl class="grid grid-cols-[6rem_1fr] gap-x-3 gap-y-1 text-sm">
          <dt class="text-muted">Host</dt>
          <dd class="font-mono break-all text-fg">{linkAddress}</dd>
          <dt class="text-muted">Fingerprint</dt>
          <dd class="font-mono text-xs break-all text-fg">{link.fingerprint}</dd>
        </dl>
        <label class="flex items-center gap-3">
          <span class="w-24 shrink-0 text-sm text-muted">Name</span>
          <Input
            bind:value={() => name, (value) => (pairForm.name = value)}
            aria-invalid={!validName || undefined}
            disabled={busy}
            class="text-sm"
          />
        </label>
        {#if !validName}
          <p class="text-sm text-danger" role="alert">Use only letters, numbers, underscores and hyphens.</p>
        {:else if existing}
          <p class="text-xs text-subtle">Replaces the saved pairing for {name}.</p>
        {/if}
        {#if identityChanged}
          <Alert variant="warning">
            This fingerprint differs from the one saved for {name}. Continue only if the server was reinstalled or its
            identity was reset.
          </Alert>
        {/if}
        {#if addressChanged}
          <p class="text-xs text-subtle">
            Address changes from <span class="font-mono">{existing?.address}</span> to
            <span class="font-mono">{linkAddress}</span>.
          </p>
        {/if}
        <div class="flex justify-end">
          <Button type="submit" size="sm" variant="accent" disabled={busy || !validName}>
            {busy ? 'Pairing…' : existing ? `Re-pair ${name}` : `Pair ${name}`}
          </Button>
        </div>
      </div>
    {/if}

    {#if error}<p class="text-sm text-danger" role="alert">{error}</p>{/if}
  </form>
</section>
