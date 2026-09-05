# Provenance

Every artifact in this design-system package traces to evidence copied from the source project.

## Source

- Source project: **Check Ui Design Md File There** (`e98bd5dc-ce25-43fb-ac40-079fb0c84b43`), a BikesNest web prototype (kind: `prototype`, linked dir `/Users/clemento/Dev/Projects/bikesnest`).
- New design-system project: `9cd2bb4d-8637-4c6e-a2f0-b14099dfb5cb`, design-system id `user:check-ui-design-md-file-there-design-system`.

## Tokens

- All color, font, shadow, and shell values in `DESIGN.md` and `colors_and_type.css` are extracted **verbatim** from the token comment block ("Design tokens — direction: human-approachable") present in the generated screens and in `scripts/gen_d.py` (shared shell), `scripts/gen_m.py` (which reads the token block from `c1-account.html`), and `scripts/gen_m56.py`. No token was invented or approximated.
- Radius, spacing, and component shapes are observed from the rendered source HTML classes (`rounded-md/lg/xl/full`, `max-w-[72rem]`, `h-10/h-11` controls, 44px icon tiles).
- The `--border` alias maps to the source token `line` (OKLch `90% 0.006 240`); both names are documented so either convention works.

## Icons

- `build/icons/*.svg` are **Lucide** icons (lucide-static v0.462.0, ISC license), downloaded from unpkg and matching the icon names invoked in the source generator scripts (`shield`, `image`, `flag`, `git-pull-request`, `map-pin`, `search`, etc.). Source screens embed Lucide inline SVGs; the kit files do the same.

## Imagery

- `assets/imagery/` contains the seven optimized photographs shipped with the source project. They are **Pexels** photos; the license permits use without attribution and filenames retain photographer attribution:
  - `hero-bike-parking.jpg` (2000×1125) — covered bike parking rows
  - `cyclist-foggy-avenue.jpg` (1600×1069) — commuter in fog, hero-adjacent
  - `cyclist-crosswalk.jpg` (1200×1200) — cyclist crossing, square crop
  - `street-rack-mint-bike.jpg` (1200×800) — street rack with mint bike
  - `square-bike-rows.jpg` (795×1200) — indoor parking rows, portrait
  - `mtb-pair-rack.jpg` (800×1200) — two MTBs at a rack, portrait
  - `commuter-portrait.jpg` (801×1200) — commuter portrait
- No logos, wordmarks, app icons, or font files existed in the source evidence; the wordmark is set in the display font stack (Avenir Next fallback).
