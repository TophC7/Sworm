<!--
  TabBeam: animated gradient glow that sweeps across the top of an active tab.
  Place as a child inside the active tab button, positioned absolutely.

  @param variant - color variant: 'accent' (default), 'warning' (working), 'success' (done), 'danger' (error)
-->
<script lang="ts" module>
  export type BeamVariant = 'accent' | 'warning' | 'success' | 'danger'
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    variant = 'accent' as BeamVariant,
    class: className
  }: {
    variant?: BeamVariant
    class?: string
  } = $props()
</script>

<span
  class={cn('pointer-events-none absolute top-0 right-0 left-0 h-[2px] overflow-hidden', className)}
  aria-hidden="true"
>
  <span class="tab-beam-gradient" data-variant={variant}></span>
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
    animation: beam-sweep 3s ease-in-out infinite;
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

  @keyframes beam-sweep {
    0% {
      transform: translateX(-50%);
    }
    100% {
      transform: translateX(0%);
    }
  }
</style>
