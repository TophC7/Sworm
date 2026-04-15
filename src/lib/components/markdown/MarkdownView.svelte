<script lang="ts">
  import SvelteMarkdown from '@humanspeak/svelte-markdown'
  import { backend } from '$lib/api/backend'
  import { Button } from '$lib/components/ui/button'
  import { TabsRoot, TabsList, TabsTrigger } from '$lib/components/ui/tabs'
  import { ResizableHandle, ResizablePane, ResizablePaneGroup } from '$lib/components/ui/resizable'
  import { TooltipRoot, TooltipTrigger, TooltipContent } from '$lib/components/ui/tooltip'
  import ContentToolbar from '$lib/components/ContentToolbar.svelte'

  type Mode = 'preview' | 'split'

  let {
    filePath,
    projectPath
  }: {
    filePath: string
    projectPath: string
  } = $props()

  let content = $state('')
  let editContent = $state('')
  let loading = $state(true)
  let saving = $state(false)
  let error = $state<string | null>(null)
  let mode = $state<Mode>('split')
  let dirty = $derived(editContent !== content)

  // Debounce preview updates in split mode so the markdown parser doesn't
  // re-run on every keystroke — noticeable on large documents.
  let debouncedEdit = $state('')
  let debounceTimer: ReturnType<typeof setTimeout> | null = null
  $effect(() => {
    if (debounceTimer) clearTimeout(debounceTimer)
    const snapshot = editContent
    debounceTimer = setTimeout(() => {
      debouncedEdit = snapshot
    }, 150)
    return () => {
      if (debounceTimer) clearTimeout(debounceTimer)
    }
  })

  let previewSource = $derived(mode === 'split' ? debouncedEdit : content)

  async function load() {
    loading = true
    error = null
    try {
      content = await backend.files.read(projectPath, filePath)
      editContent = content
      debouncedEdit = content
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    } finally {
      loading = false
    }
  }

  async function save() {
    if (!dirty) return
    saving = true
    error = null
    try {
      await backend.files.write(projectPath, filePath, editContent)
      content = editContent
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    } finally {
      saving = false
    }
  }

  function handleKeydown(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key === 's') {
      e.preventDefault()
      save()
    }
  }

  // Scroll sync — pointer tracking prevents feedback loops, rAF prevents layout thrashing
  let editorEl = $state<HTMLTextAreaElement | null>(null)
  let previewEl = $state<HTMLDivElement | null>(null)
  let activePane = $state<'editor' | 'preview' | null>(null)
  let syncScheduled = false

  function syncScroll(source: 'editor' | 'preview') {
    if (activePane !== source || syncScheduled) return
    syncScheduled = true

    requestAnimationFrame(() => {
      syncScheduled = false
      const from = source === 'editor' ? editorEl : previewEl
      const to = source === 'editor' ? previewEl : editorEl
      if (!from || !to) return

      const maxFrom = from.scrollHeight - from.clientHeight
      const maxTo = to.scrollHeight - to.clientHeight
      if (maxFrom <= 0 || maxTo <= 0) return

      to.scrollTop = (from.scrollTop / maxFrom) * maxTo
    })
  }

  let prevFilePath = ''
  $effect(() => {
    if (filePath !== prevFilePath) {
      prevFilePath = filePath
      load()
    }
  })
</script>

{#snippet renderedMarkdown()}
  <div class="px-6 py-4 text-[0.82rem] leading-relaxed text-fg">
    <SvelteMarkdown source={previewSource}>
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
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="flex h-full flex-col overflow-hidden" onkeydown={handleKeydown}>
  <ContentToolbar>
    {#snippet left()}
      <span class="truncate text-muted">{filePath}</span>
    {/snippet}
    {#snippet right()}
      <TabsRoot
        value={mode}
        onValueChange={(v) => {
          mode = v as Mode
          if (v === 'split') {
            editContent = content
            debouncedEdit = content
          }
        }}
      >
        <TabsList>
          <TabsTrigger value="preview">Preview</TabsTrigger>
          <TabsTrigger value="split">Split</TabsTrigger>
        </TabsList>
      </TabsRoot>

      {#if dirty}
        <TooltipRoot>
          <TooltipTrigger>
            {#snippet child({ props })}
              <Button variant="ghost" size="xs" onclick={save} disabled={saving} {...props}>
                {saving ? 'Saving...' : 'Save'}
              </Button>
            {/snippet}
          </TooltipTrigger>
          <TooltipContent>
            Save <kbd class="ml-2 font-mono text-[0.68rem] text-subtle">Ctrl+S</kbd>
          </TooltipContent>
        </TooltipRoot>
      {/if}
    {/snippet}
  </ContentToolbar>

  {#if error}
    <div class="px-3 py-2 text-[0.75rem] text-danger">{error}</div>
  {/if}

  <!-- Content -->
  <div class="min-h-0 flex-1">
    {#if loading}
      <div class="px-4 py-3 text-[0.75rem] text-subtle">Loading&hellip;</div>
    {:else if mode === 'preview'}
      <div class="h-full overflow-y-auto">
        {@render renderedMarkdown()}
      </div>
    {:else}
      <ResizablePaneGroup direction="horizontal">
        <ResizablePane defaultSize={50} minSize={20}>
          <textarea
            class="h-full w-full resize-none border-none bg-ground p-4 font-mono text-[0.78rem] leading-relaxed text-fg focus:outline-none"
            bind:this={editorEl}
            bind:value={editContent}
            onscroll={() => syncScroll('editor')}
            onpointerenter={() => (activePane = 'editor')}
            spellcheck="false"
          ></textarea>
        </ResizablePane>
        <ResizableHandle />
        <ResizablePane defaultSize={50} minSize={20}>
          <!-- svelte-ignore a11y_no_static_element_interactions -->
          <div
            class="h-full overflow-y-auto border-l border-edge"
            bind:this={previewEl}
            onscroll={() => syncScroll('preview')}
            onpointerenter={() => (activePane = 'preview')}
          >
            {@render renderedMarkdown()}
          </div>
        </ResizablePane>
      </ResizablePaneGroup>
    {/if}
  </div>
</div>
