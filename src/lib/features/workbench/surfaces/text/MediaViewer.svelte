<script lang="ts">
  // Desktop asset:// URL → WebView fetches direct from disk: zero IPC overhead
  // and full HTTP-Range support, so <video> seek works. Requires
  // app.security.assetProtocol enabled in src-tauri/tauri.conf.json.
  import type { MediaKind } from '$lib/features/editor/languageMap'
  import { platform } from '$lib/platform'
  import { mediaAssetUrl } from '$lib/utils/mediaAssets'

  let {
    folderPath,
    filePath,
    kind
  }: {
    folderPath: string
    filePath: string
    kind: MediaKind
  } = $props()

  let assetUrl = $derived(platform.capabilities.localAssetUrls ? mediaAssetUrl(folderPath, filePath) : null)
</script>

<div class="flex h-full w-full items-center justify-center overflow-auto bg-ground p-4">
  {#if assetUrl === null}
    <p class="text-muted">Local media is unavailable in this browser.</p>
  {:else if kind === 'image'}
    <img src={assetUrl} alt={filePath} class="max-h-full max-w-full object-contain" draggable="false" />
  {:else if kind === 'audio'}
    <audio src={assetUrl} controls class="w-full max-w-xl"></audio>
  {:else if kind === 'video'}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video src={assetUrl} controls class="max-h-full max-w-full"></video>
  {/if}
</div>
