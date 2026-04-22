<script lang="ts">
  let {
    counts,
    color = 'var(--color-accent)'
  }: {
    counts: number[]
    color?: string
  } = $props()

  const dayLetters = ['S', 'M', 'T', 'W', 'T', 'F', 'S']
  const today = new Date().getDay() // 0=Sun
  const days = Array.from({ length: 7 }, (_, i) => dayLetters[(today - 6 + i + 7) % 7])

  let maxCount = $derived(Math.max(...counts, 1))
</script>

<div class="grid grid-cols-7 gap-px">
  {#each counts as count, i}
    {@const intensity = count > 0 ? 0.25 + 0.75 * (count / maxCount) : 0}
    <div class="flex flex-col items-center gap-0.5">
      <div
        class="h-2.5 w-full rounded-[3px]"
        style="background: {count > 0 ? color : 'var(--color-edge)'}; opacity: {count > 0 ? intensity : 0.15};"
      ></div>
      <span class="text-3xs text-subtle">{days[i]}</span>
    </div>
  {/each}
</div>
