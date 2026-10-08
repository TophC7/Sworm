<!--
  TabBeam: accent or custom-color glow along a horizontal active tab edge or loading track.
  @param position - which edge to pin the beam to: 'top' (default) / 'bottom'.
-->
<script lang="ts" module>
  export type BeamPosition = 'top' | 'bottom'

  export const POSITION_CLASS: Record<BeamPosition, string> = {
    top: 'inset-x-0 top-0 h-[2px]',
    bottom: 'inset-x-0 bottom-0 h-[2px]'
  }
</script>

<script lang="ts">
  import { cn } from '$lib/utils/cn'

  let {
    position = 'top' as BeamPosition,
    color,
    class: className
  }: {
    position?: BeamPosition
    color?: string
    class?: string
  } = $props()
</script>

<span
  class={cn('pointer-events-none absolute overflow-hidden', POSITION_CLASS[position], className)}
  aria-hidden="true"
>
  <span
    class="tab-beam-gradient"
    data-variant={color ? 'custom' : 'accent'}
    style={color ? `--beam-color: ${color};` : undefined}
  ></span>
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
    animation: beam-sweep-x 3s ease-in-out infinite;
  }

  /* -- Accent (default peach) -- */
  .tab-beam-gradient[data-variant='accent'] {
    --beam-base: var(--color-accent);
    --beam-dim: var(--color-accent-dim);
    --beam-bright: var(--color-accent-bright);
    --beam-peak: var(--color-max);
  }

  /* -- Custom color (e.g. project path color) -- */
  .tab-beam-gradient[data-variant='custom'] {
    --beam-base: var(--beam-color);
    --beam-dim: color-mix(in srgb, var(--beam-color) 60%, transparent);
    --beam-bright: color-mix(in srgb, var(--beam-color) 70%, white);
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

</style>
