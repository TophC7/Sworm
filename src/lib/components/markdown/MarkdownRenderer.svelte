<script lang="ts">
  import SvelteMarkdown from '@humanspeak/svelte-markdown'

  let { source }: { source: string } = $props()
</script>

<div class="px-6 py-4 text-[0.82rem] leading-relaxed text-fg">
  <SvelteMarkdown {source}>
    {#snippet heading({ depth, children })}
      {#if depth === 1}
        <h1 class="mt-6 mb-3 border-b border-edge pb-1.5 text-[1.5rem] font-bold text-bright first:mt-0">
          {@render children?.()}
        </h1>
      {:else if depth === 2}
        <h2 class="mt-5 mb-2.5 border-b border-edge pb-1 text-[1.2rem] font-semibold text-bright first:mt-0">
          {@render children?.()}
        </h2>
      {:else if depth === 3}
        <h3 class="mt-4 mb-2 text-[1rem] font-semibold text-bright first:mt-0">{@render children?.()}</h3>
      {:else}
        <h4 class="mt-3 mb-1.5 text-[0.9rem] font-medium text-bright first:mt-0">{@render children?.()}</h4>
      {/if}
    {/snippet}

    {#snippet paragraph({ children })}
      <p class="mb-3">{@render children?.()}</p>
    {/snippet}

    {#snippet link({ href, children })}
      <a {href} class="text-accent underline decoration-accent/40 hover:decoration-accent">{@render children?.()}</a>
    {/snippet}

    {#snippet strong({ children })}
      <strong class="font-semibold text-bright">{@render children?.()}</strong>
    {/snippet}

    {#snippet em({ children })}
      <em>{@render children?.()}</em>
    {/snippet}

    {#snippet codespan({ raw })}
      <code class="rounded bg-raised px-1 py-0.5 font-mono text-[0.78em] text-accent">{raw}</code>
    {/snippet}

    {#snippet code({ text })}
      <pre class="my-3 overflow-x-auto rounded-md border border-edge bg-surface p-3"><code
          class="font-mono text-[0.78rem] text-fg">{text}</code
        ></pre>
    {/snippet}

    {#snippet blockquote({ children })}
      <blockquote class="my-3 border-l-2 border-accent/40 pl-3 text-muted">{@render children?.()}</blockquote>
    {/snippet}

    {#snippet list({ ordered, children })}
      {#if ordered}
        <ol class="mb-3 list-decimal pl-6">{@render children?.()}</ol>
      {:else}
        <ul class="mb-3 list-disc pl-6">{@render children?.()}</ul>
      {/if}
    {/snippet}

    {#snippet listitem({ children }: { children?: any })}
      <li class="mb-1">{@render children?.()}</li>
    {/snippet}

    {#snippet hr()}
      <hr class="my-6 border-edge" />
    {/snippet}

    {#snippet table({ children })}
      <div class="my-3 overflow-x-auto">
        <table class="w-full border-collapse">{@render children?.()}</table>
      </div>
    {/snippet}

    {#snippet tablehead({ children }: { children?: any })}
      <thead>{@render children?.()}</thead>
    {/snippet}

    {#snippet tablebody({ children }: { children?: any })}
      <tbody>{@render children?.()}</tbody>
    {/snippet}

    {#snippet tablerow({ children }: { children?: any })}
      <tr>{@render children?.()}</tr>
    {/snippet}

    {#snippet tablecell({ header, children }: { header: boolean; children?: any })}
      {#if header}
        <th class="border border-edge bg-surface px-3 py-1.5 text-left text-[0.78rem] font-semibold text-bright"
          >{@render children?.()}</th
        >
      {:else}
        <td class="border border-edge px-3 py-1.5">{@render children?.()}</td>
      {/if}
    {/snippet}

    {#snippet image({ href, text })}
      <img src={href} alt={text} class="my-3 max-w-full rounded" />
    {/snippet}
  </SvelteMarkdown>
</div>
