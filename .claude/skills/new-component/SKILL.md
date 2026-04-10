---
name: new-component
description: Scaffold a new Svelte component for ADE. Use when creating reusable UI or terminal/project-specific frontend components.
disable-model-invocation: true
argument-hint: "[component-name] [components|terminal|project|settings]"
---

# Create New Component

Create a new Svelte component: `$0` in `src/lib/$1/`.

## Before Creating

1. Check `src/lib/components/` for an existing reusable component first.
2. Check `src/lib/terminal/` if this is terminal-specific UI.
3. Check the current route or feature area before creating a new top-level abstraction.

If an existing component covers most of the need, extend it instead of creating a new one.

## Placement

- `src/lib/components/` for reusable UI
- `src/lib/terminal/` for terminal-related UI/helpers
- `src/routes/...` only when the code is route-specific and not reusable

## Conventions

- Use Svelte 5 runes (`$state`, `$derived`, `$effect`, `$props`, `$bindable`)
- Keep frontend components thin; privileged work belongs in Rust commands/services
- Match the current styling approach in the repo instead of inventing a separate design system
- Add `<!-- @component -->` doc comment at the top
- Use the **comment-style** skill for all comments
- Use the **svelte** MCP server to verify framework correctness

## Template

```svelte
<!--
  @component
  ComponentName -- brief description of what it does

  @param propName - what this prop controls
-->

<script lang="ts">
  // PROPS //

  let { propName = $bindable() }: { propName: string } = $props()

  // STATE //

  let internalState = $state(false)
</script>

<div class="...">
  <!-- component markup -->
</div>
```
