<script lang="ts">
  // Desktop asset:// URL → WebView fetches direct from disk: zero IPC overhead
  // and full HTTP-Range support, so <video> seek works. Requires
  // app.security.assetProtocol enabled in src-tauri/tauri.conf.json.
  // Web streams the whole file into a Blob URL; playback/seek starts after download.
  import type { MediaKind } from '$lib/features/editor/languageMap'
  import { getErrorMessage } from '$lib/features/notifications/runNotifiedTask'
  import { platform, type AssetHandle } from '$lib/platform'

  let {
    folderPath,
    filePath,
    kind
  }: {
    folderPath: string
    filePath: string
    kind: MediaKind
  } = $props()

  const assets = platform.assets
  const nativeUrl = $derived('url' in assets ? assets.url(folderPath, filePath) : null)
  let loadedUrl = $state<string | null>(null)
  let error = $state<string | null>(null)
  const assetUrl = $derived(nativeUrl ?? loadedUrl)

  $effect(() => {
    const folder = folderPath
    const file = filePath
    error = null
    loadedUrl = null
    if (!('load' in assets)) return

    const controller = new AbortController()
    let handle: AssetHandle | null = null
    assets.load(folder, file, controller.signal).then(
      (loaded) => {
        if (controller.signal.aborted) return loaded.dispose()
        handle = loaded
        loadedUrl = loaded.url
      },
      (cause: unknown) => {
        if (!controller.signal.aborted) error = getErrorMessage(cause)
      }
    )
    return () => {
      controller.abort()
      handle?.dispose()
    }
  })

  function onMediaError(event: Event) {
    const target = event.currentTarget
    error = (target instanceof HTMLMediaElement && target.error?.message) || 'The file could not be decoded'
  }
</script>

<div class="flex h-full w-full items-center justify-center overflow-auto bg-ground p-4">
  {#if error !== null}
    <p role="alert" class="text-danger">Could not load media: {error}</p>
  {:else if assetUrl === null}
    <p class="text-muted">Loading media…</p>
  {:else if kind === 'image'}
    <img
      src={assetUrl}
      alt={filePath}
      class="max-h-full max-w-full object-contain"
      draggable="false"
      onerror={onMediaError}
    />
  {:else if kind === 'audio'}
    <audio src={assetUrl} controls class="w-full max-w-xl" onerror={onMediaError}></audio>
  {:else if kind === 'video'}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video src={assetUrl} controls class="max-h-full max-w-full" onerror={onMediaError}></video>
  {/if}
</div>
