<!--
  @component
  RebindDialog: records, edits, removes, and resets shortcuts for one command.

  @param commandId - stable command id to update
  @param commandLabel - visible command label
  @param defaultSpec - legacy single default shortcut
  @param defaultKeybindings - default shortcut list
  @param onClose - close callback
-->

<script lang="ts">
  import { DialogRoot, DialogContent, DialogTitle, DialogFooter } from '$lib/components/ui/dialog'
  import { Button } from '$lib/components/ui/button'
  import { Kbd, KbdGroup } from '$lib/components/ui/kbd'
  import {
    clearShortcutOverride,
    getEffectiveBindings,
    removeCommandKeybinding,
    setCommandKeybindings
  } from '$lib/features/command-palette/shortcuts/overrides.svelte'
  import { findShortcutConflict } from '$lib/features/command-palette/shortcuts/registry.svelte'
  import { formatShortcut } from '$lib/features/command-palette/shortcuts/spec'
  import { suspendKeybindings } from '$lib/features/command-palette/shortcuts/keybindings.svelte'
  import ShortcutPreview from '$lib/features/command-palette/shortcuts/ShortcutPreview.svelte'
  import { logicalKey } from '$lib/utils/keyboardEvent'

  let {
    open = false,
    commandId,
    commandLabel,
    defaultSpec,
    defaultKeybindings = defaultSpec ? [defaultSpec] : [],
    onClose
  }: {
    open?: boolean
    commandId: string
    commandLabel: string
    defaultSpec?: string | undefined
    defaultKeybindings?: string[]
    onClose: () => void
  } = $props()

  let captured = $state<string | null>(null)
  let recording = $state(false)
  let replaceIndex = $state(0)
  let rejectedHint = $state<string | null>(null)
  let chordTimer: ReturnType<typeof setTimeout> | null = null
  let releaseRecord: (() => void) | null = null
  let releaseDialog: (() => void) | null = null

  let currentBindings = $derived(getEffectiveBindings(commandId, defaultKeybindings))
  let conflict = $derived(captured ? findShortcutConflict(commandId, captured) : null)

  function clearChordTimer() {
    if (!chordTimer) return
    clearTimeout(chordTimer)
    chordTimer = null
  }

  function startRecording(index = currentBindings.length > 0 ? 0 : -1) {
    replaceIndex = index
    captured = null
    rejectedHint = null
    recording = true
    clearChordTimer()
    releaseRecord?.()
    releaseRecord = suspendKeybindings()
  }

  function stopRecording() {
    recording = false
    clearChordTimer()
    releaseRecord?.()
    releaseRecord = null
  }

  function prettyKey(key: string): string {
    if (key === ' ') return 'Space'
    if (key === 'Escape') return 'Esc'
    if (key === '+') return '+'
    if (key.length === 1) return key.toUpperCase()
    return key
  }

  function strokeFromEvent(e: KeyboardEvent): string {
    const key = logicalKey(e)
    const mods: string[] = []
    if (e.ctrlKey || e.metaKey) mods.push('Ctrl')
    if (e.shiftKey && key !== '+') mods.push('Shift')
    if (e.altKey) mods.push('Alt')
    return [...mods, prettyKey(key)].join('+')
  }

  function handleKeydown(e: KeyboardEvent) {
    if (!recording) return
    const key = logicalKey(e)
    if (key === 'Control' || key === 'Meta' || key === 'Shift' || key === 'Alt') return
    e.preventDefault()
    e.stopPropagation()

    if (key === 'Escape') {
      stopRecording()
      return
    }

    if (key === 'Enter' && captured) {
      stopRecording()
      return
    }

    const stroke = strokeFromEvent(e)
    const hasModifier = e.ctrlKey || e.metaKey || e.altKey || (e.shiftKey && key !== '+')
    const isFunctionKey = /^F\d+$/i.test(key)
    if (!hasModifier && !isFunctionKey) {
      rejectedHint = prettyKey(key)
      return
    }

    rejectedHint = null
    if (!captured) {
      captured = formatShortcut(stroke)
      clearChordTimer()
      chordTimer = setTimeout(stopRecording, 1200)
      return
    }

    captured = formatShortcut(`${captured} ${stroke}`)
    stopRecording()
  }

  function nextBindings(): string[] {
    const nextCaptured = captured
    if (!nextCaptured) return currentBindings
    if (replaceIndex >= 0 && currentBindings.length > 0) {
      return currentBindings.map((binding, index) => (index === replaceIndex ? nextCaptured : binding))
    }
    return [...currentBindings, nextCaptured]
  }

  function save(replaceConflict = false) {
    if (!captured) return
    if (conflict && !replaceConflict) return
    if (conflict && replaceConflict) {
      removeCommandKeybinding(conflict.command.id, conflict.key, conflict.command.defaultKeybindings)
    }
    setCommandKeybindings(commandId, nextBindings())
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
  <DialogContent class="max-w-lg">
    <DialogTitle>{commandLabel}</DialogTitle>

    <div class="space-y-3 pt-4">
      <div class="space-y-2">
        {#if currentBindings.length > 0}
          {#each currentBindings as binding, index (binding)}
            <div class="flex items-center justify-between gap-3 rounded-lg border border-edge bg-overlay px-3 py-2">
              <ShortcutPreview spec={binding} />
              <div class="flex items-center gap-1.5">
                <Button size="xs" variant="ghost" onclick={() => startRecording(index)}>Edit</Button>
                <Button
                  size="xs"
                  variant="ghost"
                  onclick={() => removeCommandKeybinding(commandId, binding, defaultKeybindings)}
                >
                  Remove
                </Button>
              </div>
            </div>
          {/each}
        {:else}
          <div class="rounded-lg border border-dashed border-edge bg-surface px-3 py-3 text-sm text-subtle">
            No shortcut assigned.
          </div>
        {/if}
      </div>

      <button
        type="button"
        aria-label={recording ? 'Recording shortcut' : 'Add shortcut'}
        onclick={() => startRecording(-1)}
        class="flex min-h-[92px] w-full cursor-pointer appearance-none flex-col items-center justify-center gap-2
               rounded-xl border-2 border-dashed bg-ground/60 p-4 transition-colors
               focus-visible:shadow-focus-ring focus-visible:outline-none
               {recording ? 'border-accent bg-accent/10' : 'border-edge hover:border-accent/60 hover:bg-ground/80'}"
      >
        {#if recording}
          {#if rejectedHint}
            <KbdGroup class="text-lg">
              <Kbd class="h-8 min-w-8 px-2 text-md opacity-60">{rejectedHint}</Kbd>
            </KbdGroup>
            <span class="text-sm text-danger">Needs a modifier or function key</span>
          {:else if captured}
            <ShortcutPreview spec={captured} large={true} />
            <span class="text-xs text-subtle">Press another combo for a chord, or Enter to keep this.</span>
          {:else}
            <span class="font-mono text-base tracking-wide text-accent">Press a combo&hellip; (Esc to cancel)</span>
          {/if}
        {:else if captured}
          <ShortcutPreview spec={captured} large={true} />
        {:else}
          <span class="text-base text-subtle italic">Click to record a new shortcut</span>
        {/if}
      </button>

      {#if conflict}
        <div class="rounded-lg border border-warning/40 bg-warning-bg px-3 py-2 text-sm text-warning-bright">
          Conflicts with {conflict.command.label}.
        </div>
      {/if}
    </div>

    <DialogFooter>
      <Button variant="ghost" onclick={resetToDefault}>Reset to Default</Button>
      <Button variant="outline" onclick={close}>Cancel</Button>
      {#if conflict}
        <Button variant="accent" onclick={() => save(true)} disabled={!captured}>Replace</Button>
      {:else}
        <Button variant="accent" onclick={() => save(false)} disabled={!captured}>Save</Button>
      {/if}
    </DialogFooter>
  </DialogContent>
</DialogRoot>
