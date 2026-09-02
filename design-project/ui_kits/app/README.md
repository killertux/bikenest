# BikeNest UI Kit — applied interface kit

A live, token-bound component kit reflecting the source BikeNest prototype. Open `index.html` in a browser to browse; every page binds `../../../colors_and_type.css` (the verbatim source token block).

## Structure

```
ui_kits/app/
  index.html               Kit overview (App shell) with hero imagery and nav to all sections
  components/
    buttons.html           Primary/secondary/danger/ghost buttons, sizes, disabled, status chips, filter chips
    forms.html             Composer-style inputs, select, textarea, hints/errors, radio cards, auth field pair
    cards.html             PreviewCard (parking spot), moderation decision card, queue table, icon tiles
    navigation.html        Sidebar/tab navigation, sticky header, avatar menu, breadcrumbs
  README.md                This file
```

## Usage workflow

1. Start from `index.html` and pick the section matching the element you need.
2. Copy markup and the page-local styles; tokens come from `colors_and_type.css` — never hardcode colors.
3. For full-screen composition, choose the shell (public / authenticated / moderator) from the preserved source screens and reuse these components inside it.
4. Verify interaction states (hover, focus-visible, disabled) before delivery — the kit pages demonstrate each state's correct color pairing.

## Reuse guide

- **Component files:** the four section pages in `components/` each ship self-contained markup with page-local styles; copy a block, keep the `data-od-id` attributes.
- **Shell choice:** for full screens, pick public / authenticated / moderator from the source root screens and drop kit components inside.
- **Token discipline:** bind `../../../colors_and_type.css` first; never introduce raw colors in page styles.
- **Named building blocks:** the kit shell (`index.html`), `Sidebar`-style moderator tabs (`components/navigation.html`), `Composer`-style form stacks (`components/forms.html`), and `PreviewCard`-style spot cards (`components/cards.html`) map one-to-one onto the preserved source patterns.

## Design notes

- One solid primary button per action per viewport; secondary/danger/ghost demonstrate the remaining hierarchy.
- Semantic colors encode data status only: `fresh` (approved), `aging` (open), `stale` (unverified), `danger` (rejected/destructive).
- Chips pair a 10–12% tint with the matching strong text color; contrast holds ≥ 4.5:1 in every state.
- Hover shifts background only (neutral wash or darker solid fill); text color never lightens.
- Photos use scrim badges at 0.78 alpha anchored to the top-left corner; intrinsic aspect ratios are declared.

## Source basis

Component shapes mirror the preserved source screens at the project root: `p3-parking-details.html` (cards, forms), `m2-photos.html` (radio cards, queues, scrim badges), `m3-reports.html` (reports table), `a1-register.html` (auth fields), `p1-landing.html` (header). Imagery comes from `assets/imagery/` (Pexels-licensed, see `context/provenance.md`). Typography and colors are the source token system: Avenir Next display, system body, SF Mono data, OKLch hue-240 neutrals with the hue-170 green-teal accent.
