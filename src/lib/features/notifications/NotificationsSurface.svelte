<script lang="ts">
  import { flip } from 'svelte/animate'
  import { fly } from 'svelte/transition'
  import { tick } from 'svelte'
  import { Button } from '$lib/components/ui/button'
  import NotificationItem from '$lib/features/notifications/NotificationItem.svelte'
  import {
    MAX_VISIBLE_ACTIVE_NOTIFICATIONS,
    NOTIFICATION_EMPTY_STATE_CLASS,
    NOTIFICATION_LIST_CLASS,
    NOTIFICATION_PANEL_WIDTH_CLASS,
    NOTIFICATION_SURFACE_CLASS,
    NOTIFICATION_VIEWPORT_CLASS
  } from '$lib/features/notifications/notificationUi'
  import {
    clearAllNotifications,
    dismissNotification,
    getActiveNotifications,
    getNotifications,
    isNotificationCenterOpen,
    setNotificationCenterOpen
  } from '$lib/features/notifications/state.svelte'
  import { cn } from '$lib/utils/cn'

  let notifications = $derived(getNotifications())
  let activeNotifications = $derived(getActiveNotifications())
  let expanded = $derived(isNotificationCenterOpen())
  let visibleNotifications = $derived.by(() =>
    expanded ? notifications : activeNotifications.slice(-MAX_VISIBLE_ACTIVE_NOTIFICATIONS)
  )
  let hasVisibleSurface = $derived(expanded || visibleNotifications.length > 0)
  let notificationCount = $derived(notifications.length)
  let showEmptyState = $derived(expanded && notificationCount === 0)

  let surfaceRef = $state<HTMLDivElement | null>(null)
  let viewportRef = $state<HTMLDivElement | null>(null)

  function scrollViewportToBottom(): void {
    if (viewportRef) viewportRef.scrollTop = viewportRef.scrollHeight
  }

  function closeNotificationCenter(): void {
    setNotificationCenterOpen(false)
  }

  function isToggleTarget(target: EventTarget | null): boolean {
    return target instanceof Element && target.closest('[data-notifications-toggle="true"]') !== null
  }

  function isSurfaceInteractiveTarget(target: EventTarget | null): boolean {
    return target instanceof Element && target.closest('.notifications-surface-interactive') !== null
  }

  $effect(() => {
    if (!expanded) return

    void tick().then(scrollViewportToBottom)
  })

  $effect(() => {
    if (!expanded) return

    function handlePointerDown(event: PointerEvent): void {
      const target = event.target
      if (!(target instanceof Node)) return
      if (surfaceRef?.contains(target)) return
      if (isToggleTarget(target)) return
      if (isSurfaceInteractiveTarget(target)) return
      closeNotificationCenter()
    }

    function handleKeyDown(event: KeyboardEvent): void {
      if (event.key === 'Escape') closeNotificationCenter()
    }

    document.addEventListener('pointerdown', handlePointerDown)
    document.addEventListener('keydown', handleKeyDown)

    return () => {
      document.removeEventListener('pointerdown', handlePointerDown)
      document.removeEventListener('keydown', handleKeyDown)
    }
  })
</script>

{#if hasVisibleSurface}
  <div
    id="notifications-surface"
    class={cn('pointer-events-none fixed right-2 bottom-8 z-40', NOTIFICATION_PANEL_WIDTH_CLASS)}
    aria-live="polite"
    aria-atomic="false"
  >
    <div
      bind:this={surfaceRef}
      class={cn(
        'flex flex-col justify-end rounded-xl border',
        NOTIFICATION_SURFACE_CLASS,
        !expanded && 'border-transparent bg-transparent shadow-none'
      )}
    >
      <div
        class={cn(
          'flex items-center justify-between overflow-hidden transition-[height,padding,opacity]',
          expanded ? 'h-9 border-b border-edge px-3 py-2 opacity-100' : 'h-0 px-3 py-0 opacity-0'
        )}
        aria-hidden={!expanded}
      >
        <span class="text-sm font-medium tracking-wide text-muted uppercase">
          Notifications{notificationCount > 0 ? ` (${notificationCount})` : ''}
        </span>
        {#if notificationCount > 0}
          <Button
            variant="ghost"
            size="xs"
            class={cn('h-auto px-1.5 py-0.5 text-muted', !expanded && 'pointer-events-none')}
            onclick={clearAllNotifications}
          >
            Clear all
          </Button>
        {/if}
      </div>

      <div bind:this={viewportRef} class={cn(NOTIFICATION_VIEWPORT_CLASS, !expanded && 'overflow-visible')}>
        {#if showEmptyState}
          <div class={NOTIFICATION_EMPTY_STATE_CLASS}>No notifications.</div>
        {:else}
          <div class={NOTIFICATION_LIST_CLASS}>
            {#each visibleNotifications as notification (notification.id)}
              <div
                class="pointer-events-auto"
                animate:flip
                in:fly={{ y: 16, duration: 200 }}
                out:fly={{ y: 16, duration: 150 }}
              >
                <NotificationItem {notification} onDismiss={dismissNotification} showTimestamp={expanded} />
              </div>
            {/each}
          </div>
        {/if}
      </div>
    </div>
  </div>
{/if}
