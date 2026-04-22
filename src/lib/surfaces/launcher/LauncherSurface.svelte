<script lang="ts">
  import NewSessionView from '$lib/surfaces/launcher/NewSessionView.svelte'

  let {
    onCreated
  }: {
    onCreated?: () => void
  } = $props()
</script>

<NewSessionView {onCreated} />
