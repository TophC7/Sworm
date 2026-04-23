# Sworm 🐛 

**The Agent IDE for Linux.**

Sworm is a code-first, agent-native development environment. It doesn't try to replace your agent CLIs (Claude Code, Codex, etc.) with a chat box; instead, it wraps a real IDE around them.

Sworm is a light environment for your swarm of agents and thier buggy code. Built for developers who want to leverage AI agents without losing the power of a proper file tree, editor, and Git integration.

<!-- TODO: hero screenshot -->

> **Status:** Early development. Linux/Wayland first. Expect breaking changes as the workflow evolves, and it's moving fast.

---

## Why Sworm?

Most "ADEs" want to hide your code. They push you into chat panes and frame everything around "let the AI do it." I like AI in my workflow, but I also like reading code, owning my diffs, and using the tools my editor gives me.

Sworm takes a different approach: **The CLI is the primary agentic interface.** 

Agent CLIs like Claude Code and Codex already have excellent tool ecosystems (MCP, skills, memory). Sworm gives these agents a first-class home with:

1.  **Context-Aware Terminals:** Multi-session PTYs that automatically track agent activity and link it to the IDE state.
2.  **Review-First Workflow:** Agents touch a lot of code. Sworm is built to help you review, diff, and commit those changes with confidence.
3.  **Nix-Native Reproducibility:** Your dev environment shouldn't be a mystery. Sworm uses Nix Flakes to ensure every terminal and task runs in the exact environment your project requires.

---

## Highlights

### Agent-Native Integration
- **Thread Tracking:** Deep integration with tools like **Codex** to link terminal sessions with agent conversation history.
- **Activity Mapping:** Scans your filesystem to discover where agents have been active, providing a "heatmap" of agent-driven development, helping you locate your AI projects.
- **Multi-Session PTY:** Run `claude`, `codex`, `opencode`, or standard shells in a project-scoped, persistent terminal.

### Nix-Powered Environments
- **Flake Integration:** Point Sworm at a `flake.nix` and its `devShell` becomes the environment for every shell, session, and task. 
- **Zero Configuration:** No `direnv` or manual activation required. If it's in the flake, it's in your PATH.

### Serious Git Tooling
- **Commit Graph:** A VSCode-style visual representation of your repository's history.
- **Advanced Stashing:** Per-file stashing and a dedicated stash management UI.
- **Diff Viewer:** High-performance, Monaco-based side-by-side diffs for reviewing agent-generated changes.

### High-Performance Editor
- **Monaco + Shiki:** The reliability of Monaco with the accuracy of TextMate-based syntax highlighting.
- **Extension System:** LSP support via a manifest system (built-in support for Rust, Nix, TS/JS, and more).
- **Bundled Tools:** Includes [Fresh](https://github.com/sinelaw/fresh) as an alternate in-terminal editor for quick edits.

---

## Tech Stack

Sworm is built for performance and a native feel on modern Linux:

- **Tauri v2 + Rust:** Privileged operations (PTY, Git, FS) handled by a fast, safe backend.
- **Svelte 5:** A modern, reactive frontend that stays out of the way.
- **Monaco Editor:** The editor you know and the industry standard for web-based code editing.
- **SQLite:** Reliable local state management.
- **Nix:** The foundation for reproducible development environments.

---

## Getting Started

### Nix (Recommended)

```sh
nix run github:tophc7/Sworm
```

### From Source

```sh
git clone https://github.com/tophc7/Sworm
cd Sworm
nix develop (or install all depencies)
bun install
bun run app:dev
```

## Credits

- **[Emdash](https://github.com/generalaction/emdash)** — An agent-centric IDE The initial inspiration for sworm.
- **[Fresh](https://github.com/sinelaw/fresh)** — Bundled as an alternate in-terminal editor.

## License

[AGPLv3-or-later](./LICENSE).
