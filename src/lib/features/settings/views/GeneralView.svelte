<!--
  @component
  GeneralView — appearance readout + webview zoom.

  Only "Dark" is a supported theme today; custom themes are WIP,
  so Appearance renders as a static read-only row. Zoom is applied
  immediately via the webview API and stored in ui.svelte.ts.
-->

<script lang="ts">
  import { Badge } from '$lib/components/ui/badge'
  import { Button } from '$lib/components/ui/button'
  import { getZoomLevel, zoomIn, zoomOut, zoomReset } from '$lib/features/app-shell/zoom/state.svelte'

  let zoom = $derived(getZoomLevel())
</script>

<section class="flex flex-col gap-3 border-b border-edge px-5 py-4">
  <h3 class="text-md font-semibold text-bright">Appearance</h3>

  <div class="flex items-center">
    <span class="w-36 shrink-0 text-sm text-muted">Theme</span>
    <div class="flex flex-1 items-center gap-2">
      <span class="rounded-md border border-edge bg-surface px-3 py-1.5 text-sm text-fg"> Dark </span>
      <Badge variant="muted">WIP</Badge>
    </div>
  </div>

  <p class="pl-36 text-xs text-subtle">Custom themes are in the works.</p>
</section>

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
