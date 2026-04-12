/**
 * CLI provider metadata for session creation cards.
 *
 * Text branding uses either an SVG mask (textIcon + textAspect)
 * or plain text (textLabel + textFont).
 */

export interface ProviderMeta {
	id: string;
	label: string;
	icon: string;
	gradientFrom: string;
	gradientTo: string;
	// SVG text mode
	textIcon?: string;
	textAspect?: number;
	// Plain text mode (used when textIcon is absent)
	textLabel?: string;
	textFont?: string;
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
		id: 'copilot',
		label: 'GitHub Copilot',
		icon: '/png/copilot.png',
		textLabel: 'Copilot',
		textFont: "'SF Mono', monospace",
		gradientFrom: '#a78bfa',
		gradientTo: '#4c1d95'
	},
	{
		id: 'crush',
		label: 'Crush',
		icon: '/png/crush.png',
		textLabel: 'CRUSH',
		textFont: "'Monocraft Nerd Font', monospace",
		gradientFrom: '#f0abfc',
		gradientTo: '#701a75'
	},
	{
		id: 'gemini',
		label: 'Gemini CLI',
		icon: '/svg/gemini.svg',
		textIcon: '/svg/gemini-text.svg',
		textAspect: 98 / 24,
		gradientFrom: '#60a5fa',
		gradientTo: '#1e3a5f'
	},
	{
		id: 'opencode',
		label: 'OpenCode',
		icon: '/svg/opencode.svg',
		textIcon: '/svg/opencode-text.svg',
		textAspect: 138 / 24,
		gradientFrom: '#c084fc',
		gradientTo: '#4c1d95'
	}
];

/** Direct options — shown below the "or" divider. */
export const directOptions: ProviderMeta[] = [
	{
		id: 'fresh',
		label: 'Fresh',
		icon: '/svg/fresh.svg',
		textLabel: 'Fresh',
		textFont: "'SF Mono', monospace",
		gradientFrom: '#10b981',
		gradientTo: '#064e3b'
	},
	{
		id: 'terminal',
		label: 'Terminal',
		icon: '/svg/terminal.svg',
		textLabel: 'Terminal',
		textFont: "'Monocraft Nerd Font', monospace",
		gradientFrom: '#a1a1aa',
		gradientTo: '#3f3f46'
	}
];
