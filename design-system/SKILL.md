---
name: bikenest-design-system
description: BikeNest design system — a human-approachable civic web UI with OKLch tokens, a green-teal accent, and a semantic freshness scale, extracted from the source BikeNest prototype. Use when generating any new BikeNest screen or component.
user-invocable: true
---

# SKILL — Generating BikeNest screens with this design system

## What is inside

- `DESIGN.md` — the authoritative system: theme, color, type, spacing, layout, components, motion, voice, anti-patterns.
- `colors_and_type.css` — paste-ready token block (OKLch values with hex fallbacks in comments), base typography, and core component recipes (`.btn-primary`, `.field`, `.card`, `.badge`).
- `assets/imagery/` — seven credited Pexels photographs preserved from the source project.
- `preview/` — focused review cards (colors, typography, spacing, components, brand assets).
- `ui_kits/app/` — applied interface kit: overview plus buttons/badges, forms, cards/tables, and navigation pages, all bound to the same tokens.

## Source context

Every token, pattern, and image in this package traces to the source project "Check Ui Design Md File There" (BikeNest prototype, `e98bd5dc-ce25-43fb-ac40-079fb0c84b43`). The token block was extracted verbatim from `scripts/gen_d.py` and the generated screens; provenance for imagery and icons is documented in `context/provenance.md`. Direction: **human-approachable**.

## When to use

Use this skill whenever a task requires generating a new BikeNest screen, flow, email template, or component — public discovery pages, auth flows, account settings, contribution forms, moderation queues, or error states. Do not use it for unrelated products.

## How to use

1. Read `DESIGN.md` before writing any screen.
2. Copy the token block from `colors_and_type.css` into the first `<style>`/Tailwind config (exact Tailwind config below). **Raw OKLch values appear only in the token block** — screens reference token names only.

```js
tailwind.config = { theme: { extend: {
  colors: {
    bg: 'oklch(98% 0.004 240)', surface: 'oklch(100% 0 0)',
    fg: 'oklch(20% 0.02 240)', muted: 'oklch(50% 0.018 240)',
    line: 'oklch(90% 0.006 240)', accent: 'oklch(56% 0.12 170)',
    'accent-strong': 'oklch(45% 0.11 170)', 'accent-dark': 'oklch(40% 0.10 170)',
    'accent-soft': 'oklch(56% 0.12 170 / 0.12)', scrim: 'oklch(18% 0.015 240)',
    fresh: 'oklch(58% 0.13 155)', aging: 'oklch(68% 0.12 75)',
    stale: 'oklch(55% 0.16 35)', danger: 'oklch(47% 0.17 30)',
    'danger-soft': 'oklch(47% 0.17 30 / 0.08)',
  },
  fontFamily: {
    display: ['"Avenir Next"', '-apple-system', 'BlinkMacSystemFont', 'system-ui', 'sans-serif'],
    body: ['-apple-system', 'BlinkMacSystemFont', '"SF Pro Text"', 'system-ui', 'sans-serif'],
    mono: ['ui-monospace', '"SF Mono"', 'Menlo', 'monospace'],
  },
  boxShadow: {
    card: '0 1px 2px oklch(20% 0.02 240 / 0.06), 0 8px 24px oklch(20% 0.02 240 / 0.08)',
    pop: '0 2px 6px oklch(20% 0.02 240 / 0.10), 0 16px 40px oklch(20% 0.02 240 / 0.16)',
  },
  maxWidth: { shell: '72rem' },
}}}
```

3. Choose the right shell from the applied kit (`ui_kits/app/`): **public** (nav → hero/search → content → footer), **authenticated** (account tabs, 2fr/1fr grid), **moderator** (moderation tab bar + "Signed in as Moderator" badge). Sticky header: `border-b border-line bg-[oklch(98%_0.004_240_/_0.9)] backdrop-blur`. Body: `bg-bg font-body text-fg`, shell `max-w-[72rem]`.
4. Icons: inline Lucide SVGs, `stroke="currentColor"`, `stroke-width="2"`, 16px (`h-4 w-4`) or 20px (`h-5 w-5`) — never emoji, never icon fonts.
5. Add `data-od-id="kebab-case-id"` to every region, heading, CTA, and repeated card.

## Design-system highlights

- **One accent.** Green-teal appears at most twice per screen as a solid fill; everything else uses `accent-soft` tints.
- **Semantic colors are data, not decoration.** `fresh` = approved/verified, `aging` = open/re-check, `stale` = unverified, `danger` = rejected/destructive. Statuses render as tinted `rounded-full` chips with matching strong text.
- **One primary button per action per viewport.** Hover = background shift only (`oklch(20% 0.02 240 / 0.05)` wash on neutrals, `accent-dark` / `oklch(40% 0.15 30)` on solid fills). Text never lightens on hover.
- **Type:** Avenir Next display (bold, tracking-tight, balanced wrap) + system body 15–16px/1.6 + SF Mono for counts, IDs, coordinates, timestamps. Sentence case everywhere.
- **Space:** 4px base; card padding 20–24px; shell 72rem; radius 6/8/12/16/full; hairline `line` borders with `shadow-card` (pop only for overlays).
- **Focus:** global `:focus-visible` outline 2px `oklch(45% 0.11 170)`, offset 2px — never removed.

## Verification before delivery

- No raw hex or invented OKLch values outside the token block.
- No gradient backgrounds, no emoji icons, no second solid CTA for the same action.
- Contrast intact in every state; focus-visible works on every focusable element.
- Mobile: single column, no horizontal scroll, 44px touch targets.
- Compare component shapes against the applied kit in `ui_kits/app/` (cards/forms, queues, auth forms) and the review cards in `preview/`.
