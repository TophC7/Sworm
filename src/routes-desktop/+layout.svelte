<script lang="ts">
  import '../app.css'
  import { installPlatform, platform } from '$lib/platform'
  import { desktopPlatform } from '$lib/platform/desktop'
  import { registerHostTransport } from '$lib/api/transport'
  import { desktopHostTransport } from '$lib/api/transport.desktop'
  import { setWorkbenchId } from '$lib/features/workbench/state.svelte'
  import AppShell from '$lib/features/app-shell/AppShell.svelte'
  import type { Snippet } from 'svelte'

  installPlatform(desktopPlatform)
  registerHostTransport(desktopHostTransport)
  setWorkbenchId(platform.workbench.id)

  let { children }: { children: Snippet } = $props()
</script>

<AppShell>{@render children()}</AppShell>
