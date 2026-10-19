<!--
  @component
  GeneralView — appearance readout, typography, and webview zoom.

  Only "Dark" is supported today; zoom is available when the platform
  provides native window scaling. Font names are freeform: any installed
  family works, and the bundled fonts cover names that are not installed.

  @param onSaving - flagged when the autoSaver goes busy
  @param onSaved - flagged when it goes idle
-->

<script lang="ts">
  import { platform } from '$lib/platform'
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { Input } from '$lib/components/ui/input'
  import { getZoomLevel, zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'
  import { notify } from '$lib/features/notifications/state.svelte'
  import { getDesktopSettings, saveTypographySettings } from '$lib/features/settings/state/settings.svelte'
  import { MAX_FONT_SIZE, MIN_FONT_SIZE } from '$lib/features/settings/state/typography.svelte'
  import { getErrorMessage } from '$lib/utils/client-error'
  import { onDestroy } from 'svelte'
  import { createAutoSaver } from './autoSaver'

  type StatusHook = () => void
  let { onSaving, onSaved }: { onSaving: StatusHook; onSaved: StatusHook } = $props()

  let zoom = $derived(getZoomLevel())
  let desktop = $derived(getDesktopSettings())

  let uiFont = $state('')
  let codeFont = $state('')
  // Input's bindable value falls back to '' and rejects undefined; an empty
  // number field binds back as null.
  let codeSize = $state<number | string>('')
  let terminalFont = $state('')
  let terminalSize = $state<number | string>('')

  // Seed drafts once when settings first arrive. A later backend refresh
  // must not overwrite the user's in-progress edit.
  let seeded = false
  $effect(() => {
    if (seeded || !desktop) return
    const typography = desktop.typography
    uiFont = typography.ui_font_family
    codeFont = typography.code_font_family
    codeSize = typography.code_font_size
    terminalFont = typography.terminal_font_family ?? ''
    terminalSize = typography.terminal_font_size ?? ''
    seeded = true
  })

  const saver = createAutoSaver({
    onBusyChange: (busy) => (busy ? onSaving() : onSaved())
  })

  onDestroy(() => saver.dispose())

  /** Empty or non-numeric input reads as unset; anything else clamps into the schema's range. */
  function fontSize(value: number | string | null): number | null {
    const size = value === '' || value == null ? NaN : Number(value)
    if (!Number.isFinite(size)) return null
    return Math.min(MAX_FONT_SIZE, Math.max(MIN_FONT_SIZE, Math.round(size)))
  }

  function schedule() {
    const current = getDesktopSettings()?.typography
    if (!current) return
    const next = {
      ui_font_family: uiFont.trim(),
      code_font_family: codeFont.trim(),
      code_font_size: fontSize(codeSize) ?? current.code_font_size,
      terminal_font_family: terminalFont.trim() || null,
      terminal_font_size: fontSize(terminalSize)
    }
    saver.schedule('typography', async () => {
      try {
        await saveTypographySettings(next)
      } catch (error) {
        notify.error('Failed to save typography settings', getErrorMessage(error))
      }
    })
  }
</script>

<section class="flex flex-col gap-3 border-b border-edge px-5 py-4">
  <h3 class="text-md font-semibold text-bright">Appearance</h3>

  <div class="flex items-center">
    <span class="w-36 shrink-0 text-sm text-muted">Theme</span>
    <div class="flex flex-1 items-center gap-2">
      <span class="rounded-md border border-edge bg-surface px-3 py-1.5 text-sm text-fg"> Dark </span>
      <Badge>WIP</Badge>
    </div>
  </div>

  <p class="pl-36 text-xs text-subtle">Custom themes are in the works.</p>
</section>

<section class="flex flex-col gap-3 border-b border-edge px-5 py-4">
  <h3 class="text-md font-semibold text-bright">Typography</h3>

  <label class="flex items-center">
    <span class="w-36 shrink-0 text-sm text-muted">Interface font</span>
    <Input class="w-64" bind:value={uiFont} oninput={schedule} placeholder="Lexend" disabled={!desktop} />
  </label>

  <label class="flex items-center">
    <span class="w-36 shrink-0 text-sm text-muted">Code font</span>
    <Input
      class="w-64"
      bind:value={codeFont}
      oninput={schedule}
      placeholder="Monocraft Nerd Font"
      disabled={!desktop}
    />
  </label>

  <label class="flex items-center">
    <span class="w-36 shrink-0 text-sm text-muted">Code font size</span>
    <Input
      class="w-24"
      type="number"
      min={MIN_FONT_SIZE}
      max={MAX_FONT_SIZE}
      bind:value={codeSize}
      oninput={schedule}
      disabled={!desktop}
    />
    <span class="pl-2 text-sm text-subtle">px</span>
  </label>

  <p class="pl-36 text-xs text-subtle">
    Any installed font name. Bundled fonts fill in for names that are not installed. Code font also covers diffs and
    monospace interface text.
  </p>

  <label class="flex items-center">
    <span class="w-36 shrink-0 text-sm text-muted">Terminal font</span>
    <Input
      class="w-64"
      bind:value={terminalFont}
      oninput={schedule}
      placeholder={codeFont.trim() || 'Monocraft Nerd Font'}
      disabled={!desktop}
    />
  </label>

  <label class="flex items-center">
    <span class="w-36 shrink-0 text-sm text-muted">Terminal font size</span>
    <Input
      class="w-24"
      type="number"
      min={MIN_FONT_SIZE}
      max={MAX_FONT_SIZE}
      bind:value={terminalSize}
      oninput={schedule}
      placeholder={String(fontSize(codeSize) ?? 13)}
      disabled={!desktop}
    />
    <span class="pl-2 text-sm text-subtle">px</span>
  </label>

  <p class="pl-36 text-xs text-subtle">Leave empty to follow the code font.</p>
</section>

{#if platform.native}
  <section class="flex flex-col gap-3 px-5 py-4">
    <h3 class="text-md font-semibold text-bright">Zoom</h3>

    <div class="flex items-center">
      <span class="w-36 shrink-0 text-sm text-muted">Webview scale</span>
      <div class="flex flex-1 items-center gap-1.5">
        <Button size="sm" variant="outline" onclick={zoomOut} disabled={zoom <= 0.5}>−</Button>
        <div class="w-14 text-center font-mono text-sm text-fg">
          {Math.round(zoom * 100)}%
        </div>
        <Button size="sm" variant="outline" onclick={zoomIn} disabled={zoom >= 2.0}>+</Button>
        <Button size="sm" variant="ghost" onclick={zoomReset} disabled={zoom === 1.0}>Reset</Button>
      </div>
    </div>

    <p class="pl-36 text-xs text-subtle">Useful on HiDPI displays where native px sizes feel small.</p>
  </section>
{/if}
