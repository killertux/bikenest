# BikesNest Design System

The reusable design system behind BikesNest's UI — human-approachable, cool
near-white neutrals, one green-teal accent (OKLch hue 170), and a semantic
freshness scale.

**Direction:** human-approachable · cool near-white neutrals · one green-teal
accent · semantic freshness scale · inline Lucide icons · real credited street
photography.

## What's here

| File | What it is |
| --- | --- |
| `DESIGN.md` | The authoritative system: theme, color, type, spacing, layout, components, motion, voice, anti-patterns |
| `colors_and_type.css` | Token block + base type + core component recipes (the canonical source of the tokens used in `web/static/css/input.css`) |
| `SKILL.md` | Usage contract for agents generating new BikesNest screens |
| `context/provenance.md` | Where every token, pattern, and image came from |
| `assets/imagery/` | 7 optimized Pexels photographs |
| `preview/` | Focused review cards (colors, typography, spacing, components, brand assets) |
| `ui_kits/app/` | Applied interface kit — index + `components/` |

## Start here

1. Read `DESIGN.md`.
2. Read `SKILL.md` for the generation contract and the exact Tailwind config.
3. Copy the token block from `colors_and_type.css` into any new screen.
4. Check component shapes against `ui_kits/app/` and `preview/components-buttons.html`.
5. For imagery, take files from `assets/imagery/` — copy locally, never hotlink.
6. Verify against `DESIGN.md`  anti-patterns and the SKILL verification checklist.

## Review cards

Open in a browser, in this order:

1. `preview/colors-primary.html` — full OKLch palette, semantic freshness scale, badge pairings
2. `preview/typography-specimens.html` — display/body/mono scale with live specimens and rules
3. `preview/spacing-tokens.html` — spacing units, radii, elevation (card vs pop shadows)
4. `preview/components-buttons.html` — buttons, fields, cards, badges, queue table
5. `preview/brand-assets.html` — wordmark and the 7 preserved photos
