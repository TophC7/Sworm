<script lang="ts">
  import { resolveFileIcon, resolveFolderIcon } from './fileIconMap'

  let {
    filename,
    folder = false,
    expanded = false,
    size = 14
  }: {
    filename: string
    folder?: boolean
    expanded?: boolean
    size?: number
  } = $props()

  let src = $derived(folder ? resolveFolderIcon(filename, expanded) : resolveFileIcon(filename))
</script>

<img {src} width={size} height={size} alt="" class="shrink-0" style="min-width: {size}px" />
