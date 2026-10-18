<script lang="ts">
  import * as Alert from '$lib/components/ui/alert'
  import { Button } from '$lib/components/ui/button'
  import { GitMergeIcon } from '$lib/icons/lucideExports'
  import type { GitOperation } from '$lib/types/backend'
  import { abortOperation, isOperationPending, skipOperation } from './actions.svelte'
  import { describeOperation } from './operation'

  let { folderPath, operation }: { folderPath: string; operation: GitOperation } = $props()

  const display = $derived(describeOperation(operation))
  const busy = $derived(isOperationPending(folderPath))

  async function run(kind: 'skip' | 'abort') {
    if (busy || (kind === 'skip' && !display.canSkip)) return
    await (kind === 'skip' ? skipOperation(folderPath) : abortOperation(folderPath))
  }
</script>

<Alert.Root data-slot="git-operation-strip" variant="warning" layout="banner" class="items-center">
  <GitMergeIcon size={13} class="shrink-0 text-warning" />
  <div class="min-w-0 flex-1">
    <div class="text-xs font-medium text-warning-bright">{display.headline}</div>
    {#if display.detail}
      <div class="truncate font-mono text-xs text-subtle">{display.detail}</div>
    {/if}
  </div>
  <div class="flex shrink-0 items-center gap-1">
    {#if display.canSkip}
      <Button size="xs" variant="ghost" disabled={busy} onclick={() => run('skip')}>Skip</Button>
    {/if}
    <Button size="xs" variant="destructive" disabled={busy} onclick={() => run('abort')}>Abort</Button>
  </div>
</Alert.Root>
