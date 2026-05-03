<script lang="ts">
  // asset:// URL → WebView fetches direct from disk: zero IPC overhead
  // and full HTTP-Range support, so <video> seek works. Requires
  // app.security.assetProtocol enabled in src-tauri/tauri.conf.json.
  import { convertFileSrc } from '@tauri-apps/api/core'
  import { normalizeAbsolutePath } from '$lib/utils/paths'
  import type { MediaKind } from '$lib/features/editor/languageMap'

  let {
    projectPath,
    filePath,
    kind
  }: {
    projectPath: string
    filePath: string
    kind: MediaKind
  } = $props()

  let assetUrl = $derived(convertFileSrc(normalizeAbsolutePath(`${projectPath}/${filePath}`)))
</script>

<div class="flex h-full w-full items-center justify-center overflow-auto bg-ground p-4">
  {#if kind === 'image'}
    <img src={assetUrl} alt={filePath} class="max-h-full max-w-full object-contain" draggable="false" />
  {:else if kind === 'audio'}
    <audio src={assetUrl} controls class="w-full max-w-xl"></audio>
  {:else if kind === 'video'}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video src={assetUrl} controls class="max-h-full max-w-full"></video>
  {/if}
</div>
