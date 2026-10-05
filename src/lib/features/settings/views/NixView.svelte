<!--
  @component
  NixView — Nix runtime and editor settings.

  Eval timeout governs how long Sworm waits on a flake evaluation
  or build before giving up. Cold stores and first-run flakes can
  take minutes; under-tuned values produce false "timeout" errors.

  @param onSaving - flagged when the autoSaver goes busy
  @param onSaved - flagged when it goes idle
-->

<script lang="ts">
  import { Input } from '$lib/components/ui/input'
  import LanguageSettingsView from './LanguageSettingsView.svelte'
  import type { BuiltinSettingsPage } from '$lib/types/backend'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getSettings, getSettingsHost, saveNixSettings } from '$lib/features/settings/state/settings.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { onDestroy } from 'svelte'
  import { createAutoSaver } from './autoSaver'

  type StatusHook = () => void
  let {
    definition,
    onSaving,
    onSaved
  }: {
    definition: BuiltinSettingsPage
    onSaving: StatusHook
    onSaved: StatusHook
  } = $props()

  let settings = $derived(getSettings())
  const host = getSettingsHost()
  let timeout = $state(600)

  // Seed local state once when settings first arrive. A later backend
  // refresh must not overwrite the user's in-progress edit.
  let seeded = false
  $effect(() => {
    if (seeded || !settings) return
    timeout = settings.nix.eval_timeout_secs
    seeded = true
  })

  const saver = createAutoSaver({
    onBusyChange: (busy) => (busy ? onSaving() : onSaved())
  })

  onDestroy(() => saver.dispose())

  async function flush(next: { eval_timeout_secs: number }) {
    try {
      await saveNixSettings(next, host)
    } catch (error) {
      notify.error('Save nix settings failed', getErrorMessage(error))
    }
  }

  function schedule() {
    const next = { eval_timeout_secs: Number(timeout) }
    saver.schedule('nix-eval-timeout', () => flush(next))
  }
</script>

<section class="border-b border-edge px-5 py-4">
  <div class="flex flex-col gap-3">
    <h4 class="text-sm font-semibold text-bright">Runtime</h4>

    <div class="flex items-center">
      <span class="w-36 shrink-0 text-sm text-muted">Eval timeout</span>
      <Input class="w-24 py-2" type="number" min="30" max="3600" step="30" bind:value={timeout} oninput={schedule} disabled={!settings} />
      <span class="pl-2 text-sm text-subtle">seconds</span>
    </div>

    <p class="pl-36 text-xs text-subtle">Cold stores need 300s+; first flake build may take minutes.</p>
  </div>
</section>

<LanguageSettingsView {definition} {onSaving} {onSaved} />
