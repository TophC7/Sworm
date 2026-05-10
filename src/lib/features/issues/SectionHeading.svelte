<!--
  @component
  SectionHeading. Uppercase tracker label with an underline rule.
  Used by the issue and epic detail surfaces to break up form sections.
-->

<script lang="ts">
  let { label }: { label: string } = $props()
</script>

<div class="flex items-center gap-2">
  <span class="text-2xs font-medium tracking-wider text-muted uppercase">{label}</span>
  <span class="h-px flex-1 bg-edge"></span>
</div>
