<script lang="ts" module>
  // Emitted as hashed files rather than inlined, so ~400 icons stay out of the JS bundle.
  const iconUrls = import.meta.glob<string>('../assets/bearded/*.svg', {
    query: '?no-inline',
    import: 'default',
    eager: true
  })
</script>

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

  let name = $derived(folder ? resolveFolderIcon(filename, expanded) : resolveFileIcon(filename))
  let src = $derived(iconUrls[`../assets/bearded/${name}.svg`] ?? iconUrls['../assets/bearded/file.svg'])
</script>

<img {src} width={size} height={size} alt="" class="shrink-0" style="min-width: {size}px" />
