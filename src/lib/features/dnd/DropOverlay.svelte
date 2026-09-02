<script lang="ts">
  let { visible = false, label = '' }: { visible?: boolean; label?: string } = $props()
</script>

{#if visible}
  <div class="pointer-events-none absolute inset-0 z-30 p-2">
    <div
      class="pointer-events-none flex h-full w-full items-center justify-center rounded-xl border border-accent/60 bg-accent/20 shadow-[inset_0_0_0_1px_rgba(255,181,159,0.16)] ring-1 ring-accent/30"
    >
      {#if label}
        <span
          class="rounded border border-edge-strong/70 bg-raised/85 px-2 py-0.5 text-xs font-medium tracking-wide text-bright uppercase"
        >
          {label}
        </span>
      {/if}
    </div>
  </div>
{/if}
