<script lang="ts">
  import NewSessionView from '$lib/features/workbench/surfaces/launcher/NewSessionView.svelte'

  let {
    onCreated
  }: {
    onCreated?: () => void
  } = $props()
</script>

<NewSessionView {onCreated} />
