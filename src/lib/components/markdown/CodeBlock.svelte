<script lang="ts">
  import { highlightCode } from './highlightCode'

  let { text, lang }: { text: string; lang?: string } = $props()

  let html = $state<string | null>(null)
  let gen = 0

  $effect(() => {
    const thisGen = ++gen
    highlightCode(text, lang).then((result) => {
      if (gen === thisGen) html = result
    })
  })
</script>

{#if html}
  <!-- Shiki provides its own <pre><code> structure; wrapper handles border/rounding -->
  <div class="shiki-block my-3 overflow-x-auto rounded-md border border-edge">
    {@html html}
  </div>
{:else}
  <pre class="my-3 overflow-x-auto rounded-md border border-edge bg-surface p-4"><code
      class="font-mono text-[0.78rem] leading-[1.45] text-fg">{text}</code
    ></pre>
{/if}
