<!--
  @component
  NixView — Nix evaluator tuning.

  Eval timeout governs how long Sworm waits on a flake evaluation
  or build before giving up. Cold stores and first-run flakes can
  take minutes; under-tuned values produce false "timeout" errors.

  @param onSaving - flagged when the autoSaver goes busy
  @param onSaved - flagged when it goes idle
-->

<script lang="ts">
  import { Input } from '$lib/components/ui/input'
  import { notify } from '$lib/stores/notifications.svelte'
  import { getSettings, saveGeneralSettings } from '$lib/stores/settings.svelte'
  import { getErrorMessage } from '$lib/utils/notifiedTask'
  import { onDestroy } from 'svelte'
  import { createAutoSaver } from './autoSaver'

  type StatusHook = () => void
  let { onSaving, onSaved }: { onSaving: StatusHook; onSaved: StatusHook } = $props()

  let settings = $derived(getSettings())
  let timeout = $state(600)

  // Seed local state once when settings first arrive. A later backend
  // refresh must not overwrite the user's in-progress edit.
  let seeded = false
  $effect(() => {
    if (seeded || !settings) return
    timeout = settings.general.nix_eval_timeout_secs
    seeded = true
  })

  const saver = createAutoSaver({
    onBusyChange: (busy) => (busy ? onSaving() : onSaved())
  })

  onDestroy(() => saver.dispose())

  async function flush() {
    const current = settings?.general
    if (!current) return
    try {
      await saveGeneralSettings({ ...current, nix_eval_timeout_secs: Number(timeout) })
    } catch (error) {
      notify.error('Save nix settings failed', getErrorMessage(error))
    }
  }

  function schedule() {
    saver.schedule('nix-eval-timeout', flush)
  }
</script>

<section class="flex flex-col gap-3 px-5 py-4">
  <h3 class="text-md font-semibold text-bright">Evaluation</h3>

  <div class="flex items-center">
    <span class="w-36 shrink-0 text-sm text-muted">Eval timeout</span>
    <Input class="w-24 py-2" type="number" min="30" max="3600" step="30" bind:value={timeout} oninput={schedule} />
    <span class="pl-2 text-sm text-subtle">seconds</span>
  </div>

  <p class="pl-36 text-xs text-subtle">Cold stores need 300s+; first flake build may take minutes.</p>
</section>
