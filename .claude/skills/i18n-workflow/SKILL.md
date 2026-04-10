---
name: i18n-workflow
description: Lightweight i18n workflow guidance. Use only if ADE adopts an i18n system and you are adding or changing user-facing strings across locales.
---

# i18n Workflow

ADE does not currently define an i18n stack in the core specs.

Use this skill only when:

- the project has already adopted an i18n library and locale file structure
- you are updating user-facing strings that must stay consistent across locales

If no i18n system exists yet:

- do not invent one ad hoc
- ask first or follow the current project decision

## Rules

- Keep user-facing strings centralized once i18n exists.
- Update all required locale files together.
- Keep key naming consistent and predictable.
- State which locale keys were added or changed.

## Checklist

1. Confirm the active i18n system and locale file locations.
2. Add or update keys in every required locale file.
3. Replace hardcoded user-facing strings only where the project already expects i18n usage.
4. Report the exact keys changed so they can be reviewed.
