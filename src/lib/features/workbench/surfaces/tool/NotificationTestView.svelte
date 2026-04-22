<script lang="ts">
  import { onDestroy } from 'svelte'
  import PanelHeader from '$lib/components/layout/PanelHeader.svelte'
  import { Button } from '$lib/components/ui/button'
  import { Input, Textarea } from '$lib/components/ui/input'
  import { BellIcon } from '$lib/icons/lucideExports'
  import {
    dismissNotification,
    notify,
    setNotificationCenterOpen,
    type NotificationUpdate
  } from '$lib/features/notifications/state.svelte'

  let customTitle = $state('Notification preview')
  let customDescription = $state('This is a test notification from the tester tab.')

  const timers = new Set<number>()

  function trackTimeout(callback: () => void, delay: number): number {
    const id = window.setTimeout(() => {
      timers.delete(id)
      callback()
    }, delay)
    timers.add(id)
    return id
  }

  function trackInterval(callback: () => void, delay: number): number {
    const id = window.setInterval(callback, delay)
    timers.add(id)
    return id
  }

  function clearTimer(id: number): void {
    window.clearTimeout(id)
    window.clearInterval(id)
    timers.delete(id)
  }

  function openNotificationCenter(): void {
    setNotificationCenterOpen(true)
  }

  function scheduleNotificationUpdate(id: string, delay: number, update: NotificationUpdate): void {
    trackTimeout(() => notify.update(id, update), delay)
  }

  function createOpenCenterAction() {
    return {
      label: 'Open Center',
      onSelect: openNotificationCenter
    }
  }

  function createDismissAction(id: string | (() => string)) {
    const getId = typeof id === 'function' ? id : () => id

    return {
      label: 'Dismiss',
      onSelect: () => dismissNotification(getId())
    }
  }

  function sendBasicNotifications(): void {
    notify.info('Heads up', 'This is a neutral notification.')
    notify.success('All done', 'The operation completed successfully.')
    notify.warning('Check this', 'Something needs your attention.')
    notify.error('Something failed', 'This is what an error notification looks like.')
  }

  function sendIndeterminateLoading(): void {
    const id = notify.loading('Syncing repository', 'Waiting for remote status...')

    scheduleNotificationUpdate(id, 3000, {
      tone: 'success',
      title: 'Repository synced',
      description: 'Remote status is up to date.',
      loading: false
    })
  }

  function sendProgressNotification(): void {
    let progress = 0
    const id = notify.loading('Building workspace', 'Compiling modules...', { progress })

    const intervalId = trackInterval(() => {
      progress += 12
      if (progress >= 100) {
        clearTimer(intervalId)
        notify.update(id, {
          title: 'Build complete',
          description: 'All modules compiled successfully.',
          tone: 'success',
          loading: false
        })
        return
      }

      notify.update(id, {
        description: `Compiling modules... ${progress}%`,
        progress
      })
    }, 450)
  }

  function sendActionNotification(): void {
    let id = ''
    id = notify.info('Action required', 'This notification has two inline actions.', {
      primaryAction: createOpenCenterAction(),
      secondaryAction: createDismissAction(() => id)
    })
  }

  function sendSplitActionNotification(): void {
    notify.warning('Deploy options', 'Primary action includes a dropdown for alternate paths.', {
      primaryAction: {
        kind: 'menu',
        label: 'Deploy',
        onSelect: () => notify.success('Deploy started', 'Running the default deployment.'),
        items: [
          {
            label: 'Deploy to staging',
            onSelect: () => notify.info('Deploying to staging', 'Started staging deployment.')
          },
          {
            label: 'Deploy to production',
            onSelect: () => notify.warning('Production deploy', 'Started production deployment.')
          }
        ]
      },
      secondaryAction: {
        label: 'Open Center',
        onSelect: openNotificationCenter
      }
    })
  }

  function sendFailureRecoveryNotification(): void {
    const id = notify.loading('Uploading artifacts', 'Starting transfer...')

    scheduleNotificationUpdate(id, 1800, {
      title: 'Upload failed',
      description: 'Connection dropped before the transfer finished.',
      tone: 'error',
      loading: false,
      primaryAction: {
        label: 'Retry',
        onSelect: () => {
          notify.update(id, {
            title: 'Retrying upload',
            description: 'Attempting the transfer again...',
            tone: 'neutral',
            loading: true,
            primaryAction: null,
            secondaryAction: null
          })

          scheduleNotificationUpdate(id, 2200, {
            title: 'Upload complete',
            description: 'Artifacts uploaded successfully.',
            tone: 'success',
            loading: false
          })
        }
      },
      secondaryAction: createDismissAction(id)
    })
  }

  function sendCustomNotification(): void {
    notify.info(customTitle, customDescription, { primaryAction: createOpenCenterAction() })
  }

  onDestroy(() => {
    for (const id of timers) clearTimer(id)
  })
</script>

<div class="flex h-full min-h-0 flex-col bg-ground">
  <PanelHeader>
    {#snippet left()}
      <BellIcon size={12} class="text-accent" />
      <span class="text-xs font-semibold tracking-wide text-muted uppercase">Notification Tester</span>
    {/snippet}
    {#snippet right()}
      <Button variant="ghost" size="xs" class="h-6" onclick={openNotificationCenter}>Open Center</Button>
    {/snippet}
  </PanelHeader>

  <div class="min-h-0 flex-1 overflow-y-auto px-4 py-4">
    <div class="mx-auto flex w-full max-w-4xl flex-col gap-4">
      <section class="rounded-xl border border-edge bg-surface p-4">
        <h2 class="text-base font-semibold text-bright">Preset Scenarios</h2>
        <p class="mt-1 text-sm leading-relaxed text-muted">
          These buttons exercise the loading, progress, update, action, and split-action flows.
        </p>

        <div class="mt-4 grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
          <Button variant="default" size="sm" onclick={sendBasicNotifications}>Basic Tones</Button>
          <Button variant="default" size="sm" onclick={sendIndeterminateLoading}>Loading to Success</Button>
          <Button variant="default" size="sm" onclick={sendProgressNotification}>Progress Update</Button>
          <Button variant="default" size="sm" onclick={sendActionNotification}>Inline Actions</Button>
          <Button variant="default" size="sm" onclick={sendSplitActionNotification}>Split Action</Button>
          <Button variant="default" size="sm" onclick={sendFailureRecoveryNotification}>Retry Flow</Button>
        </div>
      </section>

      <section class="rounded-xl border border-edge bg-surface p-4">
        <h2 class="text-base font-semibold text-bright">Custom Notification</h2>
        <p class="mt-1 text-sm leading-relaxed text-muted">
          Quick way to preview copy changes without touching production code paths.
        </p>

        <div class="mt-4 grid gap-3">
          <label class="grid gap-1">
            <span class="text-xs font-medium tracking-wide text-muted uppercase">Title</span>
            <Input bind:value={customTitle} />
          </label>

          <label class="grid gap-1">
            <span class="text-xs font-medium tracking-wide text-muted uppercase">Description</span>
            <Textarea rows={3} bind:value={customDescription} />
          </label>

          <div class="flex gap-2">
            <Button variant="default" size="sm" onclick={sendCustomNotification}>Send Custom Notification</Button>
            <Button variant="ghost" size="sm" onclick={openNotificationCenter}>Show Notification Center</Button>
          </div>
        </div>
      </section>
    </div>
  </div>
</div>
