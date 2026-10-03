/**
 * CLI provider metadata for session creation cards.
 *
 * Text branding uses either an SVG mask (textIcon + textAspect)
 * or plain text (textLabel + textFont).
 */

import antigravityTextUrl from '$lib/assets/providers/antigravity-text.svg?url'
import antigravityUrl from '$lib/assets/providers/antigravity.svg?url'
import claudeCodeTextUrl from '$lib/assets/providers/claudecode-text.svg?url'
import claudeCodeUrl from '$lib/assets/providers/claudecode.svg?url'
import codexTextUrl from '$lib/assets/providers/codex-text.svg?url'
import codexUrl from '$lib/assets/providers/codex.svg?url'
import ompUrl from '$lib/assets/providers/omp.svg?url'
import terminalUrl from '$lib/assets/providers/terminal.svg?url'
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
    icon: claudeCodeUrl,
    textIcon: claudeCodeTextUrl,
    textAspect: 91 / 11,
    gradientFrom: '#f29d84',
    gradientTo: '#763724'
  },
  {
    id: 'codex',
    label: 'Codex',
    icon: codexUrl,
    textIcon: codexTextUrl,
    textAspect: 91 / 24,
    gradientFrom: '#6ee7b7',
    gradientTo: '#065f46'
  },
  {
    id: 'omp',
    label: 'OMP',
    icon: ompUrl,
    textLabel: 'OMP',
    textFont: 'var(--font-plantin)',
    gradientFrom: '#ed4abf',
    gradientTo: '#3b0764'
  },
  {
    id: 'antigravity',
    label: 'Antigravity',
    icon: antigravityUrl,
    textIcon: antigravityTextUrl,
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
    icon: terminalUrl,
    textLabel: 'Terminal',
    textFont: MONO_FONT_FAMILY,
    gradientFrom: '#a1a1aa',
    gradientTo: '#3f3f46'
  }
]

/** Every catalog entry by id. */
export const providerById: Partial<Record<string, ProviderMeta>> = Object.fromEntries(
  [...allProviders, ...directOptions].map((provider) => [provider.id, provider])
)
