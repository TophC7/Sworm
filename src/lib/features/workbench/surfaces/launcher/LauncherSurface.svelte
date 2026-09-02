<script lang="ts">
  import NewSessionView from '$lib/features/workbench/surfaces/launcher/NewSessionView.svelte'

  let { folderPath }: { folderPath: string } = $props()
</script>

<NewSessionView {folderPath} />
