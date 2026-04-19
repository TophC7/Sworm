<script lang="ts">
  import { DialogRoot, DialogContent, DialogTitle, DialogFooter } from '$lib/components/ui/dialog'
  import { Button } from '$lib/components/ui/button'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import { clearShortcutOverride, getShortcutOverride, setShortcutOverride } from '$lib/stores/shortcutOverrides.svelte'
  import { splitShortcut, suspendKeybindings } from '$lib/utils/keybindings.svelte'

  let {
    open = false,
    commandId,
    commandLabel,
    defaultSpec,
    onClose
  }: {
    open?: boolean
    commandId: string
    commandLabel: string
    defaultSpec: string | undefined
    onClose: () => void
  } = $props()

  // `captured` is the new combo the user just pressed (pretty-cased,
  // e.g. "Ctrl+Shift+T"). While the dialog is open we fully suspend
  // global shortcut dispatch — otherwise pressing e.g. Ctrl+Shift+R
  // while the dialog's up would reload the window. Escape is handled
  // by the Dialog primitive natively, so it still closes.
  let captured = $state<string | null>(null)
  let recording = $state(false)
  // When the user presses a plain-letter key during capture we reject
  // it but echo it back with a hint, so the capture square doesn't
  // feel unresponsive.
  let rejectedHint = $state<string | null>(null)
  let releaseRecord: (() => void) | null = null
  let releaseDialog: (() => void) | null = null

  let override = $derived(getShortcutOverride(commandId))

  // What to show in the capture square:
  //   recording → the live "press a combo" prompt
  //   captured  → the just-pressed combo
  //   otherwise → the override (or default) that's currently bound
  let displaySpec = $derived(captured ?? override ?? defaultSpec)
  let displayParts = $derived(splitShortcut(displaySpec))

  function startRecording() {
    if (recording) return
    captured = null
    rejectedHint = null
    recording = true
    // Nested suspend on top of the dialog-level one; both release
    // independently. Kept as a separate token so re-record toggles
    // don't bleed into the dialog's own suspension lifetime.
    releaseRecord?.()
    releaseRecord = suspendKeybindings()
  }

  function stopRecording() {
    recording = false
    releaseRecord?.()
    releaseRecord = null
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!recording) return
    if (e.key === 'Control' || e.key === 'Meta' || e.key === 'Shift' || e.key === 'Alt') return
    e.preventDefault()
    e.stopPropagation()
    if (e.key === 'Escape') {
      stopRecording()
      return
    }
    const mods: string[] = []
    if (e.ctrlKey || e.metaKey) mods.push('Ctrl')
    if (e.shiftKey) mods.push('Shift')
    if (e.altKey) mods.push('Alt')

    const key = e.key.length === 1 ? e.key.toUpperCase() : e.key
    const isFunctionKey = /^F\d+$/i.test(e.key)
    // Plain letters without modifiers are too easy to collide with
    // everyday typing — require at least one modifier unless the base
    // key is a function key. Echo the rejected press so the user sees
    // their input was received, just not accepted.
    if (mods.length === 0 && !isFunctionKey) {
      rejectedHint = key
      return
    }

    captured = [...mods, key].join('+')
    rejectedHint = null
    stopRecording()
  }

  function save() {
    if (!captured) return
    setShortcutOverride(commandId, captured)
    close()
  }

  function resetToDefault() {
    clearShortcutOverride(commandId)
    close()
  }

  function close() {
    stopRecording()
    captured = null
    onClose()
  }

  // Dialog-level suspend: every global shortcut (reload, new terminal,
  // palette toggle, …) is silenced for as long as the dialog is open.
  // Escape still works because the Dialog primitive binds it natively.
  //
  // Acquire on the open=true branch; cleanup owns the release so the
  // token lifecycle stays linear (one acquire, one release per open).
  $effect(() => {
    if (!open) {
      stopRecording()
      captured = null
      return
    }
    releaseDialog = suspendKeybindings()
    return () => {
      releaseDialog?.()
      releaseDialog = null
    }
  })
</script>

<svelte:window onkeydown={handleKeydown} />

<DialogRoot
  {open}
  onOpenChange={(v) => {
    if (!v) close()
  }}
>
  <DialogContent class="max-w-md">
    <DialogTitle>{commandLabel}</DialogTitle>
    <div class="pt-4">
      <button
        type="button"
        aria-label={recording ? 'Recording — press a key combo' : 'Click to record a new shortcut'}
        onclick={startRecording}
        class="flex min-h-[96px] w-full cursor-pointer appearance-none flex-col items-center justify-center gap-2
               rounded-xl border-2 border-dashed bg-ground/60 p-4 transition-colors
               focus-visible:shadow-focus-ring focus-visible:outline-none
               {recording ? 'border-accent bg-accent/10' : 'border-edge hover:border-accent/60 hover:bg-ground/80'}"
      >
        {#if recording}
          {#if rejectedHint}
            <KbdGroup class="text-lg">
              <Kbd class="h-8 min-w-8 px-2 text-md opacity-60">{rejectedHint}</Kbd>
            </KbdGroup>
            <span class="text-sm text-danger">Needs a modifier (Ctrl / Shift / Alt)</span>
          {:else}
            <span class="font-mono text-base tracking-wide text-accent">Press a combo&hellip; (Esc to cancel)</span>
          {/if}
        {:else if displayParts.length > 0}
          <KbdGroup class="text-lg">
            {#each displayParts as part, i (i)}
              {#if i > 0}<span class="text-subtle">+</span>{/if}
              <Kbd class="h-8 min-w-8 px-2 text-md">{part}</Kbd>
            {/each}
          </KbdGroup>
        {:else}
          <span class="text-base text-subtle italic">Click to record</span>
        {/if}

        {#if defaultSpec && defaultSpec !== displaySpec}
          <span class="text-xs text-subtle">
            Default: <span class="font-mono">{defaultSpec}</span>
          </span>
        {/if}
      </button>
    </div>
    <DialogFooter>
      <Button variant="ghost" onclick={resetToDefault}>Reset to Default</Button>
      <Button variant="outline" onclick={close}>Cancel</Button>
      <Button variant="accent" onclick={save} disabled={!captured}>Save</Button>
    </DialogFooter>
  </DialogContent>
</DialogRoot>
