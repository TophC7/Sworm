<!--
  TabBeam: animated gradient glow that sweeps along the edge of an active
  tab (or, vertically, the side of an active row). Also re-used by
  NotificationProgressBar for non-accent progress tones.

  @param variant - color variant. Tabs always use 'accent' per design spec §7.3.
                    Other variants exist only for notification progress bars.
  @param position - which edge to pin the beam to: 'top' (default) / 'bottom'
                    run horizontal; 'left' / 'right' run vertical (full height).
-->
<script lang="ts" module>
  export type BeamVariant = 'accent' | 'warning' | 'success' | 'danger'
  export type BeamPosition = 'top' | 'bottom' | 'left' | 'right'
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    variant = 'accent' as BeamVariant,
    position = 'top' as BeamPosition,
    class: className
  }: {
    variant?: BeamVariant
    position?: BeamPosition
    class?: string
  } = $props()

  const POSITION_CLASS: Record<BeamPosition, string> = {
    top: 'inset-x-0 top-0 h-[2px]',
    bottom: 'inset-x-0 bottom-0 h-[2px]',
    left: 'inset-y-0 left-0 w-[2px]',
    right: 'inset-y-0 right-0 w-[2px]'
  }

  let vertical = $derived(position === 'left' || position === 'right')
</script>

<span
  class={cn('pointer-events-none absolute overflow-hidden', POSITION_CLASS[position], className)}
  aria-hidden="true"
>
  <span class="tab-beam-gradient" data-variant={variant} data-orientation={vertical ? 'vertical' : 'horizontal'}></span>
</span>

<style>
  .tab-beam-gradient {
    position: absolute;
    inset: 0;
    background: var(--beam-base);
  }

  .tab-beam-gradient::after {
    content: '';
    position: absolute;
    top: 0;
    left: 0;
  }

  /* -- Horizontal sweep (top / bottom) -- */
  .tab-beam-gradient[data-orientation='horizontal']::after {
    width: 200%;
    height: 100%;
    background: linear-gradient(
      90deg,
      transparent 0%,
      var(--beam-dim) 20%,
      var(--beam-bright) 40%,
      var(--beam-peak) 50%,
      var(--beam-bright) 60%,
      var(--beam-dim) 80%,
      transparent 100%
    );
    animation: beam-sweep-x 3s ease-in-out infinite;
  }

  /* -- Vertical sweep (left / right) -- */
  .tab-beam-gradient[data-orientation='vertical']::after {
    width: 100%;
    height: 200%;
    background: linear-gradient(
      180deg,
      transparent 0%,
      var(--beam-dim) 20%,
      var(--beam-bright) 40%,
      var(--beam-peak) 50%,
      var(--beam-bright) 60%,
      var(--beam-dim) 80%,
      transparent 100%
    );
    animation: beam-sweep-y 3s ease-in-out infinite;
  }

  /* -- Accent (default peach) -- */
  .tab-beam-gradient[data-variant='accent'] {
    --beam-base: var(--color-accent);
    --beam-dim: var(--color-accent-dim);
    --beam-bright: var(--color-accent-bright);
    --beam-peak: var(--color-max);
  }

  /* -- Warning (yellow, agent working) -- */
  .tab-beam-gradient[data-variant='warning'] {
    --beam-base: var(--color-warning);
    --beam-dim: color-mix(in srgb, var(--color-warning) 60%, transparent);
    --beam-bright: var(--color-warning-bright);
    --beam-peak: var(--color-max);
  }

  /* -- Success (green, agent done) -- */
  .tab-beam-gradient[data-variant='success'] {
    --beam-base: var(--color-success);
    --beam-dim: color-mix(in srgb, var(--color-success) 60%, transparent);
    --beam-bright: var(--color-success-bright);
    --beam-peak: var(--color-max);
  }

  /* -- Danger (red, error) -- */
  .tab-beam-gradient[data-variant='danger'] {
    --beam-base: var(--color-danger);
    --beam-dim: color-mix(in srgb, var(--color-danger) 60%, transparent);
    --beam-bright: var(--color-danger-bright);
    --beam-peak: var(--color-max);
  }

  @keyframes beam-sweep-x {
    0% {
      transform: translateX(-50%);
    }
    100% {
      transform: translateX(0%);
    }
  }

  @keyframes beam-sweep-y {
    0% {
      transform: translateY(-50%);
    }
    100% {
      transform: translateY(0%);
    }
  }
</style>
