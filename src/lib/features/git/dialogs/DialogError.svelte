<!--
  @component
  DialogError: compact branch-dialog error text.
-->

<script lang="ts">
  let {
    message
  }: {
    message?: string | null
  } = $props()
</script>

{#if message}
  <p class="text-2xs text-danger">{message}</p>
{/if}
