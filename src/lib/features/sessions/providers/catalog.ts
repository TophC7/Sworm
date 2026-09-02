/**
 * CLI provider metadata for session creation cards.
 *
 * Text branding uses either an SVG mask (textIcon + textAspect)
 * or plain text (textLabel + textFont).
 */

import { MONO_FONT_FAMILY } from '$lib/fonts'

export interface ProviderMeta {
  id: string
  label: string
  icon: string
  gradientFrom: string
  gradientTo: string
  // SVG text mode
  textIcon?: string
  textAspect?: number
  // Plain text mode (used when textIcon is absent)
  textLabel?: string
  textFont?: string
}

/** Agent CLI providers — detected and managed by the backend. */
export const allProviders: ProviderMeta[] = [
  {
    id: 'claude_code',
    label: 'Claude Code',
    icon: '/svg/claudecode.svg',
    textIcon: '/svg/claudecode-text.svg',
    textAspect: 91 / 11,
    gradientFrom: '#f29d84',
    gradientTo: '#763724'
  },
  {
    id: 'codex',
    label: 'Codex',
    icon: '/svg/codex.svg',
    textIcon: '/svg/codex-text.svg',
    textAspect: 91 / 24,
    gradientFrom: '#6ee7b7',
    gradientTo: '#065f46'
  },
  {
    id: 'omp',
    label: 'OMP',
    icon: '/svg/omp.svg',
    textLabel: 'OMP',
    textFont: 'var(--font-plantin)',
    gradientFrom: '#ed4abf',
    gradientTo: '#3b0764'
  },
  {
    id: 'antigravity',
    label: 'Antigravity',
    icon: '/svg/antigravity.svg',
    textIcon: '/svg/antigravity-text.svg',
    textAspect: 422 / 88,
    gradientFrom: '#a78bfa',
    gradientTo: '#312e81'
  }
]

/** Direct options — shown below the "or" divider. */
export const directOptions: ProviderMeta[] = [
  {
    id: 'terminal',
    label: 'Terminal',
    icon: '/svg/terminal.svg',
    textLabel: 'Terminal',
    textFont: MONO_FONT_FAMILY,
    gradientFrom: '#a1a1aa',
    gradientTo: '#3f3f46'
  }
]
