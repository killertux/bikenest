# BikeNest Design System Package

A reusable design-system package extracted from the **BikeNest** prototype (source project "Check Ui Design Md File There", project `e98bd5dc-ce25-43fb-ac40-079fb0c84b43`).

## Product Overview

BikeNest is a community-maintained web application for finding bike parking. Cyclists search a shared map of parking spots, view photos and freshness data, add new spots, propose changes, and write reviews. Moderators review submitted photos, reports, and proposals, and an audit log keeps the community accountable. The app includes crowdsourced listings with photo evidence, per-record freshness tracking (fresh / aging / stale), moderator approve/reject workflows with reason capture, and transparent community history, designed so contributors can keep every listing current.

**Primary surfaces:** public discovery (landing, search, spot details, legal pages), authentication (register/login/verify/reset), account management (profile, favorites, contributions, privacy, export), contribution forms (add spot, propose change, review), moderation queues (photos, reports, proposals, users, audit), and error states — 33 screens in total, all preserved in this workspace.

**Core capabilities:** crowdsourced listings with photo evidence, per-record freshness tracking (fresh / aging / stale), moderator approve/reject workflows with reason capture, and transparent community history.

**Direction:** human-approachable · cool near-white neutrals · one green-teal accent (OKLch hue 170) · semantic freshness scale · inline Lucide icons · real credited street photography.

## Start here

| File | What it is |
| --- | --- |
| `DESIGN.md` | The authoritative system: theme, color, type, spacing, layout, components, motion, voice, anti-patterns |
| `colors_and_type.css` | Paste-ready token block + base type + core component recipes |
| `SKILL.md` | Usage contract for agents generating new BikeNest screens |
| `context/provenance.md` | Where every token, pattern, and image came from |
| `context/source-context.md` | Original source-project handoff |

## Package contents

```
DESIGN.md                  System documentation (read first)
colors_and_type.css        Tokens, base typography, component recipes
README.md                  This file
SKILL.md                   Agent-facing usage contract
assets/
  imagery/                 7 optimized Pexels photographs preserved from the source project
build/
  icons/                   25 Lucide runtime icons (ISC) used by the source screens
preview/                   Focused review cards (see manifest below)
ui_kits/app/               Applied interface kit — index + components/ + README
context/                   Source handoff + provenance
images/                    Original (unoptimized) source evidence — untouched
scripts/                   Source screen generators — primary token evidence
p*.html, a*.html, c*.html, d*.html, m*.html, e*.html   Preserved source screens (33)
render-checks/             Rendered verification snapshots of source screens
```

No `fonts/` directory ships: Avenir Next is a system font in the source evidence and no webfont files exist to preserve. No logotype files existed either; the wordmark is set in the display stack.

## Preview manifest

Review the generated preview cards in this order:

1. `preview/colors-primary.html` — full OKLch palette, semantic freshness scale, badge pairings
2. `preview/typography-specimens.html` — display/body/mono scale with live specimens and rules
3. `preview/spacing-tokens.html` — spacing units, radii, elevation (card vs pop shadows)
4. `preview/components-buttons.html` — buttons, fields, cards, badges, queue table
5. `preview/brand-assets.html` — wordmark, all 7 preserved photos, all 25 Lucide icons
6. `preview/applied-ui.html` — render-check thumbnails plus live iframes of four source screens

## Reuse Workflow

1. Read `DESIGN.md`, then `SKILL.md` for the generation contract and exact Tailwind config.
2. Copy the token block from `colors_and_type.css` into the new screen.
3. Pick the matching shell (public / authenticated / moderator) from the preserved source screens.
4. Check component shapes against `ui_kits/app/` (`index.html` plus `components/buttons.html`, `components/forms.html`, `components/cards.html`, `components/navigation.html`) and `preview/components-buttons.html`.
5. For imagery or icons, take files from `assets/imagery/` and `build/icons/` — copy locally, never hotlink.
6. Verify against `DESIGN.md` §9 anti-patterns and the SKILL verification checklist.

## Preserved source examples

The complete BikeNest prototype is preserved untouched at the workspace root and remains the highest-signal reference for component shapes and copy tone: public `p1`–`p7`, auth `a1`–`a5`, account `c1`–`c7`, contribution `d1`–`d3`, moderation `m1`–`m6`, errors `e1`–`e2`.
