#!/usr/bin/env python3
"""Generate D1/D2/D3 contribution form pages with the shared authenticated shell."""
import re, json, pathlib

ICONS = pathlib.Path("assets/vendor/icons")
_cache = {}

def body(name):
    if name not in _cache:
        raw = (ICONS / f"{name}.svg").read_text()
        _cache[name] = re.search(r"<svg[^>]*>(.*)</svg>", raw, re.S).group(1)
    return _cache[name]

def icon(name, cls="lucide h-4 w-4"):
    return (f'<svg aria-hidden="true" class="{cls}" xmlns="http://www.w3.org/2000/svg" '
            f'width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" '
            f'stroke-width="2" stroke-linecap="round" stroke-linejoin="round">{body(name)}</svg>')

# ---------------------------------------------------------------- shell
HEAD = """<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{{TITLE}} — BikeNest</title>
  <meta name="description" content="{{DESC}}" />
  <meta name="robots" content="noindex" />
  <script src="https://cdn.tailwindcss.com"></script>
  <script>
    /* Design tokens — direction: human-approachable. Raw values live only here. */
    tailwind.config = {
      theme: {
        extend: {
          colors: {
            bg:      'oklch(98% 0.004 240)',
            surface: 'oklch(100% 0 0)',
            fg:      'oklch(20% 0.02 240)',
            muted:   'oklch(50% 0.018 240)',
            line:    'oklch(90% 0.006 240)',
            accent:  'oklch(56% 0.12 170)',
            'accent-strong': 'oklch(45% 0.11 170)',
            'accent-dark':   'oklch(40% 0.10 170)',
            'accent-soft':   'oklch(56% 0.12 170 / 0.12)',
            'scrim': 'oklch(18% 0.015 240)',
            fresh:   'oklch(58% 0.13 155)',
            aging:   'oklch(68% 0.12 75)',
            stale:   'oklch(55% 0.16 35)',
            danger:  'oklch(47% 0.17 30)',
            'danger-soft': 'oklch(47% 0.17 30 / 0.08)',
          },
          fontFamily: {
            display: ['"Avenir Next"', '-apple-system', 'BlinkMacSystemFont', 'system-ui', 'sans-serif'],
            body:    ['-apple-system', 'BlinkMacSystemFont', '"SF Pro Text"', 'system-ui', 'sans-serif'],
            mono:    ['ui-monospace', '"SF Mono"', 'Menlo', 'monospace'],
          },
          boxShadow: {
            card: '0 1px 2px oklch(20% 0.02 240 / 0.06), 0 8px 24px oklch(20% 0.02 240 / 0.08)',
            pop:  '0 2px 6px oklch(20% 0.02 240 / 0.10), 0 16px 40px oklch(20% 0.02 240 / 0.16)',
          },
          maxWidth: { shell: '72rem' },
        }
      }
    }
  </script>
  <style type="text/tailwindcss">
    @layer base {
      html { -webkit-text-size-adjust: 100%; }
      body { text-rendering: optimizeLegibility; -webkit-font-smoothing: antialiased; }
      p { text-wrap: pretty; }
      h1, h2, h3 { text-wrap: balance; }
      :focus-visible { outline: 2px solid oklch(45% 0.11 170); outline-offset: 2px; border-radius: 4px; }
    }
  </style>
</head>
<body class="bg-bg font-body text-fg">
"""

HEADER = """
  <!-- ============ Header (authenticated chrome) ============ -->
  <header data-od-id="topnav-account" class="sticky top-0 z-40 border-b border-line bg-[oklch(98%_0.004_240_/_0.9)] backdrop-blur">
    <div class="mx-auto flex h-16 max-w-shell items-center justify-between gap-6 px-5 lg:px-8">
      <a href="p1-landing.html" class="flex items-center gap-2.5" aria-label="BikeNest — home">
        <span class="grid h-9 w-9 place-items-center rounded-xl bg-accent text-white">{BIKE5}</span>
        <span class="font-display text-lg font-bold tracking-tight">BikeNest</span>
      </a>

      <nav class="hidden items-center gap-7 md:flex" aria-label="Primary">
        <a href="p1-landing.html#how-it-works" class="text-sm text-muted hover:text-fg">How it works</a>
        <a href="p2-search.html" class="text-sm text-muted hover:text-fg">Parking spots</a>
        <a href="p1-landing.html#community" class="text-sm text-muted hover:text-fg">Community</a>
      </nav>

      <div class="hidden items-center gap-4 md:flex">
        <div class="flex items-center rounded-lg border border-line p-0.5 font-mono text-xs" role="group" aria-label="Language">
          <a href="#" lang="pt-BR" class="rounded-md px-2 py-1 text-muted hover:text-fg" aria-label="Português (Brasil)">PT</a>
          <a href="#" lang="en" aria-current="true" class="rounded-md bg-fg px-2 py-1 font-medium text-bg" aria-label="English">EN</a>
        </div>
        <div class="relative" data-od-id="user-menu">
          <button id="user-btn" type="button" aria-expanded="false" aria-haspopup="menu"
                  class="flex items-center gap-2 rounded-full border border-line py-1 pl-1 pr-2.5 transition-colors hover:bg-[oklch(20%_0.02_240_/_0.04)]">
            <span class="grid h-8 w-8 place-items-center rounded-full bg-accent-strong font-display text-xs font-bold text-white">AR</span>
            <span class="text-sm font-medium">Ana</span>
            {CHEV}
          </button>
          <div id="user-menu" role="menu" aria-label="Account menu"
               class="hidden absolute right-0 top-[calc(100%+8px)] w-60 rounded-xl border border-line bg-surface p-1.5 shadow-pop">
            <div class="border-b border-line px-3 py-2.5">
              <p class="text-sm font-semibold">Ana Ribeiro</p>
              <p class="mt-0.5 truncate text-xs text-muted">ana.ribeiro@example.com</p>
            </div>
            <a role="menuitem" href="c1-account.html" class="mt-1 flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-fg hover:bg-[oklch(20%_0.02_240_/_0.05)]">{USER} Account overview</a>
            <a role="menuitem" href="c4-favorites.html" class="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-fg hover:bg-[oklch(20%_0.02_240_/_0.05)]">{HEART} Favorites</a>
            <a role="menuitem" href="c5-contributions.html" class="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-fg hover:bg-[oklch(20%_0.02_240_/_0.05)]">{CLIP} Contributions</a>
            <a role="menuitem" href="c6-privacy.html" class="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm text-fg hover:bg-[oklch(20%_0.02_240_/_0.05)]">{SHIELD} Privacy &amp; data</a>
            <div class="my-1 border-t border-line"></div>
            <a role="menuitem" href="a2-login.html" class="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm font-medium text-danger hover:bg-danger-soft">{LOGOUT} Log out</a>
          </div>
        </div>
      </div>

      <button id="menu-btn" class="grid h-10 w-10 place-items-center rounded-lg border border-line md:hidden" aria-expanded="false" aria-controls="mobile-menu" aria-label="Open menu">
        {MENU}
      </button>
    </div>

    <div id="mobile-menu" class="hidden border-t border-line bg-bg px-5 py-4 md:hidden">
      <nav class="flex flex-col gap-1" aria-label="Primary mobile">
        <a href="p1-landing.html#how-it-works" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">How it works</a>
        <a href="p2-search.html" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Parking spots</a>
        <a href="p1-landing.html#community" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Community</a>
      </nav>
      <div class="mt-3 flex flex-col gap-1 border-t border-line pt-3">
        <p class="px-3 pb-1 font-mono text-[11px] uppercase tracking-wide text-muted">Your account</p>
        <a href="c1-account.html" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Overview</a>
        <a href="c4-favorites.html" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Favorites</a>
        <a href="c5-contributions.html" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Contributions</a>
        <a href="c6-privacy.html" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Privacy &amp; data</a>
        <a href="a2-login.html" class="mt-1 rounded-lg px-3 py-2.5 text-[15px] font-medium text-danger hover:bg-danger-soft">Log out</a>
      </div>
    </div>
  </header>
"""

FOOTER = """
  <footer data-od-id="footer" class="mt-4 border-t border-line bg-surface">
    <div class="mx-auto max-w-shell px-5 py-12 lg:px-8">
      <div class="flex flex-col gap-10 md:flex-row md:items-start md:justify-between">
        <div class="max-w-xs">
          <a href="p1-landing.html" class="flex items-center gap-2.5" aria-label="BikeNest — home">
            <span class="grid h-8 w-8 place-items-center rounded-lg bg-accent text-white">{BIKE4}</span>
            <span class="font-display text-base font-bold tracking-tight">BikeNest</span>
          </a>
          <p class="mt-3 text-sm leading-relaxed text-muted">A community-maintained map of bicycle parking, built by and for cyclists.</p>
        </div>

        <nav class="grid grid-cols-2 gap-x-12 gap-y-2 text-sm" aria-label="Footer">
          <a href="p7-about.html" class="py-1 text-muted hover:text-fg">About</a>
          <a href="#" class="py-1 text-muted hover:text-fg">Contact</a>
          <a href="p4-privacy.html" class="py-1 text-muted hover:text-fg">Privacy policy</a>
          <a href="p5-terms.html" class="py-1 text-muted hover:text-fg">Terms of service</a>
          <a href="p6-cookies.html" class="py-1 text-muted hover:text-fg">Cookie policy</a>
          <a href="p7-about.html#how-it-works" class="py-1 text-muted hover:text-fg">How it works</a>
        </nav>
      </div>
      <div class="mt-10 flex flex-wrap items-center justify-between gap-3 border-t border-line pt-6">
        <p class="text-xs text-muted">© 2026 BikeNest</p>
        <p class="font-mono text-xs text-muted">{{FOOTER_LABEL}}</p>
      </div>
    </div>
  </footer>

  <!-- ============ Flash (mimics the app's flash behaviour) ============ -->
  <div class="pointer-events-none fixed inset-x-0 bottom-6 z-50 flex justify-center px-5">
    <p id="flash" role="status" aria-live="polite" class="pointer-events-auto hidden max-w-md items-center gap-2.5 rounded-xl bg-fg px-4 py-3 text-sm text-bg shadow-pop">
      {INFO}
      <span id="flash-text"></span>
    </p>
  </div>

  <script>

    /* ---------- Shell: user menu, mobile menu, flash ---------- */
    var userBtn = document.getElementById('user-btn');
    var userMenu = document.getElementById('user-menu');
    userBtn.addEventListener('click', function (e) {
      e.stopPropagation();
      var open = !userMenu.classList.toggle('hidden');
      userBtn.setAttribute('aria-expanded', String(open));
    });
    document.addEventListener('click', function (e) {
      if (!userMenu.classList.contains('hidden') && !userMenu.contains(e.target) && !userBtn.contains(e.target)) {
        userMenu.classList.add('hidden');
        userBtn.setAttribute('aria-expanded', 'false');
      }
    });
    document.addEventListener('keydown', function (e) {
      if (e.key === 'Escape' && !userMenu.classList.contains('hidden')) {
        userMenu.classList.add('hidden');
        userBtn.setAttribute('aria-expanded', 'false');
        userBtn.focus();
      }
    });

    var menuBtn = document.getElementById('menu-btn');
    var mobileMenu = document.getElementById('mobile-menu');
    menuBtn.addEventListener('click', function () {
      var open = !mobileMenu.classList.toggle('hidden');
      menuBtn.setAttribute('aria-expanded', String(open));
      menuBtn.setAttribute('aria-label', open ? 'Close menu' : 'Open menu');
    });

    var flashEl = document.getElementById('flash');
    var flashText = document.getElementById('flash-text');
    var flashTimer;
    function flash(msg) {
      flashText.textContent = msg;
      flashEl.classList.remove('hidden');
      flashEl.classList.add('flex');
      clearTimeout(flashTimer);
      flashTimer = setTimeout(function () {
        flashEl.classList.add('hidden');
        flashEl.classList.remove('flex');
      }, 4600);
    }
  </script>
"""

sh = dict(
    BIKE5=icon("bike", "lucide h-5 w-5"), BIKE4=icon("bike", "lucide h-4 w-4"),
    CHEV=icon("chevron-down", "lucide h-4 w-4 text-muted"), USER=icon("user"),
    HEART=icon("heart"), CLIP=icon("clipboard-list"), SHIELD=icon("shield-check"),
    LOGOUT=icon("log-out"), MENU=icon("menu", "lucide h-5 w-5"), INFO=icon("info", "lucide h-4 w-4 shrink-0"),
)
for k, v in sh.items():
    HEADER = HEADER.replace("{" + k + "}", v)
    FOOTER = FOOTER.replace("{" + k + "}", v)

def breadcrumb(items):
    parts = []
    for i, (label, href) in enumerate(items):
        if i:
            parts.append(icon("chevron-right", "lucide h-3.5 w-3.5 text-[oklch(50%_0.018_240_/_0.6)]"))
        if href:
            parts.append(f'<a href="{href}" class="hover:text-fg hover:underline">{label}</a>')
        else:
            parts.append(f'<span class="font-medium text-fg" aria-current="page">{label}</span>')
    nav = ('<nav aria-label="Breadcrumb" class="flex flex-wrap items-center gap-1.5 text-sm text-muted">'
           + "".join(parts) + "</nav>")
    return f"""<div data-od-id="context-bar" class="border-b border-line bg-bg">
    <div class="mx-auto flex max-w-shell flex-wrap items-center justify-between gap-3 px-5 py-3 lg:px-8">
      {nav}
      <p class="hidden items-center gap-1.5 text-xs text-muted sm:flex">{icon("mail-check", "lucide h-3.5 w-3.5 text-fresh")} Email verified — you can contribute</p>
    </div>
  </div>"""

def card(open_, title, icon_name, badge=""):
    bdg = f'<span class="ml-auto">{badge}</span>' if badge else ""
    return f"""<section data-od-id="{open_}" class="rounded-2xl border border-line bg-surface p-6 shadow-card sm:p-7">
      <div class="flex flex-wrap items-center gap-2.5">
        <span class="grid h-8 w-8 place-items-center rounded-xl bg-[oklch(56%_0.12_170_/_0.10)] text-accent-strong">{icon(icon_name, "lucide h-4 w-4")}</span>
        <h2 class="font-display text-lg font-bold">{title}</h2>
        {bdg}
      </div>
      <div class="mt-5">"""

CLOSE = "</div></section>"

def field_label(for_id, text, hint=""):
    h = f' <span class="font-normal text-muted">· {hint}</span>' if hint else ""
    return f'<label for="{for_id}" class="text-sm font-medium">{text}{h}</label>'

def err(id_):
    return (f'<p id="{id_}" class="mt-1.5 hidden items-center gap-1.5 text-xs font-medium text-danger">'
            f'{icon("circle-alert", "lucide h-3.5 w-3.5 shrink-0")}<span></span></p>')

MINI_MAP = f"""
        <div id="pin-map" role="application" aria-label="Map — click or use arrow keys to move the pin" tabindex="0"
             class="relative h-64 cursor-crosshair touch-none overflow-hidden rounded-xl border border-line bg-[oklch(96%_0.006_240)] select-none">
          <svg aria-hidden="true" class="absolute inset-0 h-full w-full" viewBox="0 0 400 256" preserveAspectRatio="none">
            <rect width="400" height="256" fill="oklch(96% 0.006 240)" />
            <path d="M0 170 L400 140" stroke="oklch(88% 0.008 240)" stroke-width="12" fill="none" />
            <path d="M120 0 L150 256" stroke="oklch(88% 0.008 240)" stroke-width="10" fill="none" />
            <path d="M0 60 L400 84" stroke="oklch(91% 0.008 240)" stroke-width="7" fill="none" />
            <path d="M300 0 L286 256" stroke="oklch(91% 0.008 240)" stroke-width="7" fill="none" />
            <rect x="180" y="95" width="70" height="46" rx="4" fill="oklch(92% 0.012 140)" />
            <rect x="40" y="190" width="60" height="42" rx="4" fill="oklch(91% 0.012 240)" />
            <rect x="320" y="110" width="56" height="40" rx="4" fill="oklch(91% 0.012 240)" />
            <text x="28" y="164" font-family="ui-monospace, SF Mono, Menlo, monospace" font-size="9" fill="oklch(50% 0.018 240)">R. Domingos de Morais</text>
          </svg>
          <div id="pin" class="pointer-events-none absolute hidden -translate-x-1/2 -translate-y-full" style="left:44%; top:52%;">
            <span class="flex flex-col items-center">
              <span class="grid h-9 w-9 place-items-center rounded-full bg-accent-strong text-white shadow-pop ring-4 ring-[oklch(45%_0.11_170_/_0.18)]">{icon("map-pin", "lucide h-4 w-4")}</span>
              <span class="mt-0.5 h-2 w-2 rotate-45 rounded-[2px] bg-accent-strong"></span>
            </span>
          </div>
          <p id="pin-hint" class="absolute inset-x-0 bottom-3 mx-auto w-max rounded-full bg-[oklch(20%_0.02_240_/_0.78)] px-3.5 py-1.5 text-xs font-medium text-white">Click the map to drop the pin</p>
        </div>
        <div class="mt-2 flex flex-wrap items-center justify-between gap-2">
          <p class="font-mono text-xs text-muted">Pin: <span id="pin-coords" class="text-fg">not placed yet</span></p>
          <button type="button" id="locate-btn" class="inline-flex items-center gap-1.5 rounded-lg border border-line bg-surface px-2.5 py-1.5 text-xs font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("locate-fixed", "lucide h-3.5 w-3.5")} Use my current location</button>
        </div>
"""

SECURITY_CHIPS = "".join(
    f"""<label class="sec-opt inline-flex cursor-pointer items-center gap-2 rounded-full border border-line px-3.5 py-2 text-sm font-medium transition-colors hover:bg-[oklch(20%_0.02_240_/_0.04)] has-[:checked]:border-accent-strong has-[:checked]:bg-[oklch(56%_0.12_170_/_0.10)]">
      <input type="checkbox" value="{v}" class="accent-[oklch(45%_0.11_170)]" {chk} />{icon(ic, "lucide h-4 w-4")} {label}
    </label>"""
    for v, ic, label, chk in [
        ("staffed", "user-check", "Staffed during the day", "checked"),
        ("cctv", "video", "CCTV", "checked"),
        ("covered", "umbrella", "Covered", "checked"),
        ("lockers", "lock", "Locks / lockers", ""),
        ("controlled", "door-closed", "Access-controlled", ""),
        ("lighting", "sun", "Well lit at night", "checked"),
    ])

TYPE_CHIPS = "".join(
    f"""<label class="type-opt flex cursor-pointer flex-col items-center gap-1.5 rounded-xl border border-line px-3 py-3 text-center text-xs font-medium transition-colors hover:bg-[oklch(20%_0.02_240_/_0.04)] has-[:checked]:border-accent-strong has-[:checked]:bg-[oklch(56%_0.12_170_/_0.10)]">
      <input type="radio" name="type" value="{v}" class="sr-only" {chk} />{icon(ic, "lucide h-5 w-5")} {label}
    </label>"""
    for v, ic, label, chk in [
        ("rack", "bike", "Rack", ""),
        ("covered", "umbrella", "Covered", "checked"),
        ("staffed", "user-check", "Staffed", ""),
        ("lockers", "lock", "Lockers", ""),
    ])

ICON_REGISTRY = ("<script>\n"
                 "    /* Inline SVG bodies for dynamically rendered icons (no runtime CDN). */\n"
                 f"    window.__icBodies = {json.dumps({k: body(k) for k in ['bike', 'umbrella', 'user-check', 'lock']})};\n"
                 "    window.icIcon = function (name) {\n"
                 "      return '<svg aria-hidden=\"true\" class=\"lucide h-3 w-3\" xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 24 24\" fill=\"none\" stroke=\"currentColor\" stroke-width=\"2\" stroke-linecap=\"round\" stroke-linejoin=\"round\">' + (window.__icBodies[name] || '') + '</svg>';\n"
                 "    };\n"
                 "  </script>")

STATE_PANEL = f"""<details data-od-id="state-previews" class="mt-8 rounded-2xl border border-dashed border-[oklch(50%_0.018_240_/_0.5)] bg-[oklch(96%_0.006_240)] px-5 py-4">
  <summary class="flex cursor-pointer items-center gap-2 text-sm font-medium text-muted">{{FLASK}} Prototype-only: hidden states</summary>
  <div class="mt-4 flex flex-wrap gap-2">
{{PV_BUTTONS}}
  </div>
</details>""".replace("{FLASK}", icon("sparkles", "lucide h-4 w-4"))

def write_page(path, title, desc, bc_html, main_html, footer_label, script):
    html = (HEAD.replace("{{TITLE}}", title).replace("{{DESC}}", desc)
            + HEADER + bc_html + main_html + FOOTER.replace("{{FOOTER_LABEL}}", footer_label)
            + script + "\n</body>\n</html>\n")
    pathlib.Path(path).write_text(html)
    print(f"wrote {path} ({len(html.splitlines())} lines)")

# ================================================================= D1 — Add parking
d1_pv_buttons = f"""
    <button type="button" id="pv-unverified" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-3.5 py-2 text-xs font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("mail-check", "lucide h-3.5 w-3.5")} Unverified account block</button>
    <button type="button" id="pv-dup" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-3.5 py-2 text-xs font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("triangle-alert", "lucide h-3.5 w-3.5")} Show duplicate warning</button>
    <button type="button" id="pv-error" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-3.5 py-2 text-xs font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("circle-alert", "lucide h-3.5 w-3.5")} Show validation errors</button>
"""

d1_main = f"""
  <main id="content" data-od-id="d1-main" class="mx-auto w-full max-w-shell px-5 py-10 lg:px-8">
    <div class="mb-8 max-w-2xl">
      <p class="font-mono text-xs uppercase tracking-[0.14em] text-muted">Contribute · Add parking</p>
      <h1 class="mt-1 font-display text-3xl font-bold tracking-tight">Add a parking location</h1>
      <p class="mt-2 text-[15px] leading-relaxed text-muted">Share a spot you actually use. Precise details help other riders decide in seconds — and everything you enter goes live after a quick moderation pass.</p>
    </div>

    <div class="grid gap-8 lg:grid-cols-[minmax(0,1fr)_340px]">
      <!-- ===== Form column ===== -->
      <div>
        <form id="d1-form" novalidate>
          {card("d1-location", "Location", "map-pin")}
            <div class="grid gap-5">
              <div>
                {field_label("f-name", "Name", "a short, recognizable name")}
                <input id="f-name" name="name" type="text" autocomplete="off" placeholder="e.g. Praça Vilma Dentzi — covered rack"
                       class="mt-1.5 w-full rounded-xl border border-line bg-bg px-3.5 py-2.5 text-[15px] placeholder:text-[oklch(50%_0.018_240_/_0.55)] focus:border-accent-strong" />
                {err("e-name")}
              </div>
              <div>
                {field_label("f-address", "Street address", "we'll look up the coordinates")}
                <div class="relative mt-1.5">
                  <input id="f-address" name="address" type="text" autocomplete="off" placeholder="Start typing an address…"
                         class="w-full rounded-xl border border-line bg-bg pl-10 pr-10 py-2.5 text-[15px] placeholder:text-[oklch(50%_0.018_240_/_0.55)] focus:border-accent-strong" />
                  <span class="absolute left-3 top-1/2 -translate-y-1/2 text-muted">{icon("search", "lucide h-4 w-4")}</span>
                  <span id="geocode-spin" class="absolute right-3 top-1/2 hidden -translate-y-1/2 text-accent-strong">{icon("loader-2", "lucide h-4 w-4 animate-spin")}</span>
                </div>
                <p id="geocode-note" class="mt-1.5 hidden items-center gap-1.5 text-xs font-medium text-fresh">{icon("check-circle-2", "lucide h-3.5 w-3.5 shrink-0")} Found and placed on the map — click the map to adjust.</p>
                {err("e-address")}
              </div>
              <div>
                <p class="text-sm font-medium">Pin the exact spot <span class="font-normal text-muted">· drop it at the entrance, not the corner</span></p>
                {MINI_MAP}
                {err("e-pin")}
              </div>
            </div>
          {CLOSE}

          {card("d1-type-cost", "Type &amp; cost", "warehouse")}
            <div class="grid gap-6">
              <fieldset>
                <legend class="text-sm font-medium">Parking type</legend>
                <div class="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4" role="radiogroup" aria-label="Parking type">
                  {TYPE_CHIPS}
                </div>
                {err("e-type")}
              </fieldset>
              <fieldset>
                <legend class="text-sm font-medium">Cost</legend>
                <div class="mt-2 flex flex-wrap gap-2">
                  <label class="flex cursor-pointer items-center gap-2 rounded-xl border border-line px-3.5 py-2 text-sm font-medium transition-colors has-[:checked]:border-accent-strong has-[:checked]:bg-[oklch(56%_0.12_170_/_0.10)]">
                    <input type="radio" name="cost" value="free" class="accent-[oklch(45%_0.11_170)]" checked /> Free
                  </label>
                  <label class="flex cursor-pointer items-center gap-2 rounded-xl border border-line px-3.5 py-2 text-sm font-medium transition-colors has-[:checked]:border-accent-strong has-[:checked]:bg-[oklch(56%_0.12_170_/_0.10)]">
                    <input type="radio" name="cost" value="paid" class="accent-[oklch(45%_0.11_170)]" /> Paid
                  </label>
                </div>
                <div id="paid-fields" class="mt-3 hidden grid gap-3 sm:grid-cols-[140px_1fr]">
                  <div>
                    {field_label("f-amount", "Amount")}
                    <div class="relative mt-1.5">
                      <span class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 font-mono text-sm text-muted">R$</span>
                      <input id="f-amount" type="number" min="0" step="0.5" placeholder="5,00" class="w-full rounded-xl border border-line bg-bg py-2.5 pl-10 pr-3 text-[15px] focus:border-accent-strong" />
                    </div>
                    {err("e-amount")}
                  </div>
                  <div>
                    {field_label("f-unit", "Unit")}
                    <select id="f-unit" class="mt-1.5 w-full rounded-xl border border-line bg-bg px-3 py-2.5 text-[15px] focus:border-accent-strong">
                      <option value="hour">per hour</option>
                      <option value="day">per day</option>
                      <option value="month">per month</option>
                    </select>
                  </div>
                </div>
                <p id="free-note" class="mt-2 text-xs text-muted">Leave the amount out if riders park at no charge — moderators may confirm it.</p>
              </fieldset>
            </div>
          {CLOSE}

          {card("d1-hours", "Opening hours", "clock", f'<span class="inline-flex items-center gap-1 rounded-full bg-[oklch(56%_0.12_170_/_0.10)] px-2.5 py-1 text-xs font-medium text-accent-strong">{icon("map-pin", "lucide h-3 w-3")}<span id="tz-chip">America/Sao_Paulo</span></span>')}
            <p class="text-sm leading-relaxed text-muted">Hours are stored in the location's own timezone, derived from the pin's coordinates and confirmable below. Wall-clock ranges are kept as entered — they don't shift with daylight saving.</p>
            <div id="hours-rows" class="mt-4 divide-y divide-line rounded-xl border border-line"><!-- rows injected by JS --></div>
            <div class="mt-4 grid gap-3 sm:grid-cols-[1fr_auto] sm:items-end">
              <div>
                {field_label("f-tz", "Timezone")}
                <select id="f-tz" class="mt-1.5 w-full rounded-xl border border-line bg-bg px-3 py-2.5 text-[15px] focus:border-accent-strong">
                  <option>America/Sao_Paulo (BRT)</option>
                  <option>America/Manaus (AMT)</option>
                  <option>America/Rio_Branco (ACT)</option>
                  <option>America/Noronha (FNT)</option>
                </select>
              </div>
              <p class="text-xs leading-relaxed text-muted sm:max-w-[230px]">Auto-derived from the pin. Override only if the suggestion is wrong.</p>
            </div>
          {CLOSE}

          {card("d1-security", "Security features", "shield-check")}
            <p class="text-sm leading-relaxed text-muted">Tick only what you can personally confirm — riders rely on these signals, and they feed the freshness and verification badges.</p>
            <div class="mt-4 flex flex-wrap gap-2">
              {SECURITY_CHIPS}
            </div>
          {CLOSE}

          {card("d1-description", "Description &amp; photos", "camera")}
            <div>
              <div class="flex items-baseline justify-between">
                {field_label("f-desc", "Description", "optional, but helpful")}
                <span id="desc-count" class="font-mono text-xs text-muted">0 / 600</span>
              </div>
              <textarea id="f-desc" rows="4" maxlength="600" placeholder="Anything a rider should know: how full it gets, where the entrance is, quirks of the rack…"
                        class="mt-1.5 w-full resize-y rounded-xl border border-line bg-bg px-3.5 py-2.5 text-[15px] placeholder:text-[oklch(50%_0.018_240_/_0.55)] focus:border-accent-strong"></textarea>
            </div>
            <div class="mt-5">
              {field_label("f-photos", "Photos", "optional · up to 4")}
              <label id="dropzone" class="mt-1.5 flex cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-line bg-[oklch(98%_0.004_240)] px-6 py-8 text-center transition-colors hover:border-accent-strong hover:bg-[oklch(56%_0.12_170_/_0.06)]">
                {icon("image-plus", "lucide h-6 w-6 text-muted")}
                <span class="text-sm font-medium">Add photos of the spot</span>
                <span class="text-xs text-muted">JPG or PNG · location metadata is removed on upload (EXIF stripped)</span>
                <input id="f-photos" type="file" accept="image/*" multiple class="sr-only" />
              </label>
              <div id="thumbs" class="mt-3 hidden flex-wrap gap-3"></div>
            </div>
          {CLOSE}

          <!-- duplicate advisory -->
          <div id="dup-warning" class="mt-6 hidden rounded-2xl border border-[oklch(68%_0.12_75_/_0.55)] bg-[oklch(68%_0.12_75_/_0.12)] p-5">
            <div class="flex items-start gap-3">
              <span class="mt-0.5 text-[oklch(58%_0.14_70)]">{icon("triangle-alert", "lucide h-5 w-5 shrink-0")}</span>
              <div>
                <p class="text-sm font-semibold">Possible duplicate</p>
                <p class="mt-1 text-sm leading-relaxed text-muted">A spot called “<span class="font-medium text-fg">Estação Vila Mariana</span>” already exists about <span class="font-medium text-fg">40 m</span> from your pin, with a similar address. You can continue anyway — duplicate detection is a warning, not a block, and a moderator will compare both entries.</p>
                <a href="p3-parking-details.html" class="mt-2 inline-flex items-center gap-1.5 text-sm font-medium text-accent-strong hover:text-accent-dark">{icon("external-link", "lucide h-3.5 w-3.5")} View the existing spot</a>
              </div>
            </div>
          </div>

          <div class="mt-8 flex flex-col gap-4 border-t border-line pt-6 sm:flex-row sm:items-center sm:justify-between">
            <p class="text-xs leading-relaxed text-muted">Your email stays private — public attribution is off by default.<br />Creation is rate-limited to a few locations per day to prevent spam.</p>
            <div class="flex shrink-0 items-center gap-3">
              <a href="p2-search.html" class="rounded-xl px-4 py-2.5 text-sm font-medium text-muted hover:text-fg">Cancel</a>
              <button id="submit-btn" type="submit" class="inline-flex items-center gap-2 rounded-xl bg-accent-strong px-5 py-2.5 text-sm font-semibold text-white shadow-card transition-colors hover:bg-accent-dark disabled:cursor-wait disabled:opacity-70">
                {icon("plus", "lucide h-4 w-4")}<span id="submit-label">Publish location</span>
              </button>
            </div>
          </div>
        </form>

        <!-- success state -->
        <section id="d1-success" class="hidden rounded-2xl border border-line bg-surface p-8 text-center shadow-card sm:p-12" aria-live="polite">
          <span class="mx-auto grid h-14 w-14 place-items-center rounded-full bg-[oklch(58%_0.13_155_/_0.12)] text-fresh">{icon("check-circle-2", "lucide h-7 w-7")}</span>
          <h2 class="mt-4 font-display text-2xl font-bold">Location published</h2>
          <p class="mx-auto mt-2 max-w-md text-[15px] leading-relaxed text-muted">“<span id="success-name" class="font-medium text-fg">Your spot</span>” is now live on the map. Photos go through a quick moderation pass before appearing publicly. Coordinates, creator and timestamp were recorded automatically — your identity isn't shown on the public page.</p>
          <div class="mt-6 flex flex-wrap items-center justify-center gap-3">
            <a href="p3-parking-details.html" class="inline-flex items-center gap-2 rounded-xl bg-accent-strong px-5 py-2.5 text-sm font-semibold text-white shadow-card transition-colors hover:bg-accent-dark">View the details page {icon("arrow-right", "lucide h-4 w-4")}</a>
            <a href="d1-add-parking.html" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-5 py-2.5 text-sm font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("plus", "lucide h-4 w-4")} Add another</a>
          </div>
        </section>

        {STATE_PANEL.replace("{PV_BUTTONS}", d1_pv_buttons)}
      </div>

      <!-- ===== Rail ===== -->
      <aside class="lg:sticky lg:top-24 lg:self-start" aria-label="Live preview and tips">
        <section data-od-id="d1-preview" class="rounded-2xl border border-line bg-surface p-5 shadow-card">
          <p class="font-mono text-[11px] uppercase tracking-wide text-muted">Live preview — how riders will see it</p>
          <article class="mt-3 overflow-hidden rounded-xl border border-line">
            <div class="grid h-28 place-items-center bg-[oklch(94%_0.008_240)] text-[oklch(50%_0.018_240_/_0.45)]">{icon("bike", "lucide h-7 w-7")}</div>
            <div class="p-4">
              <div class="flex flex-wrap items-center gap-1.5">
                <span id="pv-type" class="inline-flex items-center gap-1 rounded-full bg-[oklch(56%_0.12_170_/_0.10)] px-2.5 py-0.5 text-xs font-medium text-accent-strong">{icon("umbrella", "lucide h-3 w-3")} Covered</span>
                <span class="inline-flex items-center gap-1 rounded-full border border-dashed border-[oklch(50%_0.018_240_/_0.5)] px-2.5 py-0.5 text-xs font-medium text-muted">{icon("clock-alert", "lucide h-3 w-3")} Freshness: —</span>
              </div>
              <h3 id="pv-name" class="mt-2 font-display text-[15px] font-bold leading-snug">Untitled parking spot</h3>
              <p id="pv-address" class="mt-0.5 flex items-center gap-1 text-xs text-muted">{icon("map-pin", "lucide h-3 w-3 shrink-0")}<span class="truncate">Address appears here</span></p>
              <div class="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
                <span id="pv-cost" class="font-semibold text-fresh">Free</span>
                <span class="text-muted">Rating: —</span>
                <span id="pv-sec" class="text-muted">No security details yet</span>
              </div>
            </div>
          </article>
        </section>

        <section data-od-id="d1-tips" class="mt-4 rounded-2xl border border-line bg-surface p-5 shadow-card">
          <h2 class="font-display text-[15px] font-bold">Before you add</h2>
          <ul class="mt-3 grid gap-2.5 text-sm leading-relaxed text-muted">
            <li class="flex gap-2.5"><span class="mt-0.5 shrink-0 text-fresh">{icon("check", "lucide h-4 w-4")}</span>Enter details you've seen yourself — not guesses.</li>
            <li class="flex gap-2.5"><span class="mt-0.5 shrink-0 text-fresh">{icon("check", "lucide h-4 w-4")}</span>Pin the entrance, not the street corner.</li>
            <li class="flex gap-2.5"><span class="mt-0.5 shrink-0 text-fresh">{icon("check", "lucide h-4 w-4")}</span>Photos speed up moderation and build trust.</li>
            <li class="flex gap-2.5"><span class="mt-0.5 shrink-0 text-fresh">{icon("check", "lucide h-4 w-4")}</span>Don't include private info about people or homes.</li>
          </ul>
        </section>
      </aside>
    </div>
  </main>
"""

d1_script = """
  <script>
    /* ---------- D1: add parking location ---------- */
    (function () {
      var $ = function (id) { return document.getElementById(id); };

      /* ---- opening hours rows ---- */
      var DAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
      var DEFAULTS = [
        ['06:00', '22:00', true], ['06:00', '22:00', true], ['06:00', '22:00', true],
        ['06:00', '22:00', true], ['06:00', '22:00', true], ['08:00', '20:00', true],
        ['08:00', '18:00', false]
      ];
      var hoursBox = $('hours-rows');
      DAYS.forEach(function (day, i) {
        var d = DEFAULTS[i];
        var row = document.createElement('div');
        row.className = 'flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-2.5';
        row.innerHTML =
          '<label class="flex w-28 shrink-0 items-center gap-2 text-sm">' +
          '<input type="checkbox" class="h-4 w-4 accent-[oklch(45%_0.11_170)]" ' + (d[2] ? 'checked' : '') + ' aria-label="' + day + ': open" />' + day + '</label>' +
          '<input type="time" value="' + d[0] + '" aria-label="' + day + ' opens at" class="w-28 rounded-lg border border-line bg-bg px-2.5 py-1.5 text-sm focus:border-accent-strong" />' +
          '<span class="text-xs text-muted">to</span>' +
          '<input type="time" value="' + d[1] + '" aria-label="' + day + ' closes at" class="w-28 rounded-lg border border-line bg-bg px-2.5 py-1.5 text-sm focus:border-accent-strong" />' +
          '<span class="hidden text-xs font-medium text-muted">Closed</span>';
        hoursBox.appendChild(row);
        var cb = row.querySelector('input[type=checkbox]');
        var times = row.querySelectorAll('input[type=time]');
        var closedTag = row.querySelector('span.hidden');
        function sync() {
          var open = cb.checked;
          times.forEach(function (t) { t.disabled = !open; t.classList.toggle('opacity-40', !open); });
          closedTag.classList.toggle('hidden', open);
        }
        cb.addEventListener('change', sync);
        sync();
      });

      /* ---- type radios ---- */
      var TYPE_ICON = { rack: 'bike', covered: 'umbrella', staffed: 'user-check', lockers: 'lock' };
      var TYPE_LABEL = { rack: 'Rack', covered: 'Covered', staffed: 'Staffed', lockers: 'Lockers' };
      document.querySelectorAll('input[name=type]').forEach(function (r) {
        r.addEventListener('change', function () {
          $('pv-type').innerHTML = icIcon(TYPE_ICON[r.value]) + ' ' + TYPE_LABEL[r.value];
        });
      });

      /* ---- cost ---- */
      function updatePaidPreview() {
        var amt = $('f-amount').value;
        var unit = $('f-unit').value;
        $('pv-cost').textContent = amt ? ('R$ ' + amt + ' / ' + unit) : 'Paid — amount pending';
      }
      document.querySelectorAll('input[name=cost]').forEach(function (r) {
        r.addEventListener('change', function () {
          var paid = $('f-amount') && document.querySelector('input[name=cost][value=paid]').checked;
          $('paid-fields').classList.toggle('hidden', !paid);
          $('free-note').classList.toggle('hidden', paid);
          if (paid) { updatePaidPreview(); } else { $('pv-cost').textContent = 'Free'; }
        });
      });
      $('f-amount').addEventListener('input', updatePaidPreview);
      $('f-unit').addEventListener('change', updatePaidPreview);

      /* ---- map pin ---- */
      var map = $('pin-map'), pin = $('pin'), placed = false;
      var LAT0 = -23.6105, LNG0 = -46.6392;
      function placePin(xPct, yPct) {
        placed = true;
        pin.classList.remove('hidden');
        $('pin-hint').classList.add('hidden');
        pin.style.left = xPct + '%';
        pin.style.top = yPct + '%';
        $('pin-coords').textContent = (LAT0 + (52 - yPct) * 0.0004).toFixed(4) + ', ' + (LNG0 + (xPct - 44) * 0.0004).toFixed(4);
        $('tz-chip').textContent = 'America/Sao_Paulo';
        $('e-pin').classList.add('hidden'); $('e-pin').classList.remove('flex');
      }
      map.addEventListener('click', function (e) {
        var r = map.getBoundingClientRect();
        placePin(((e.clientX - r.left) / r.width) * 100, ((e.clientY - r.top) / r.height) * 100);
      });
      map.addEventListener('keydown', function (e) {
        var x = parseFloat(pin.style.left) || 44, y = parseFloat(pin.style.top) || 52;
        if (e.key === 'ArrowLeft') { placePin(Math.max(2, x - 2), y); e.preventDefault(); }
        if (e.key === 'ArrowRight') { placePin(Math.min(98, x + 2), y); e.preventDefault(); }
        if (e.key === 'ArrowUp') { placePin(x, Math.max(6, y - 3)); e.preventDefault(); }
        if (e.key === 'ArrowDown') { placePin(x, Math.min(94, y + 3)); e.preventDefault(); }
      });
      $('locate-btn').addEventListener('click', function () {
        if (!navigator.geolocation) { flash('Location is not supported by this browser.'); return; }
        navigator.geolocation.getCurrentPosition(function () {
          placePin(52, 48);
          flash('Pin moved to your current location — adjust it if needed.');
        }, function () {
          flash('Location unavailable — drop the pin by clicking the map instead.');
        }, { timeout: 6000 });
      });

      /* ---- geocode simulation ---- */
      var addressInput = $('f-address');
      addressInput.addEventListener('blur', function () {
        if (!addressInput.value.trim()) return;
        $('geocode-spin').classList.remove('hidden');
        setTimeout(function () {
          $('geocode-spin').classList.add('hidden');
          $('geocode-note').classList.remove('hidden');
          $('geocode-note').classList.add('flex');
          if (!placed) placePin(44, 52);
        }, 700);
      });
      addressInput.addEventListener('input', function () {
        var v = addressInput.value.trim();
        $('pv-address').lastElementChild.textContent = v || 'Address appears here';
      });

      /* ---- description counter ---- */
      var desc = $('f-desc');
      desc.addEventListener('input', function () { $('desc-count').textContent = desc.value.length + ' / 600'; });

      /* ---- photos ---- */
      $('f-photos').addEventListener('change', function (e) {
        var files = Array.prototype.slice.call(e.target.files).slice(0, 4);
        var thumbs = $('thumbs');
        thumbs.innerHTML = '';
        if (!files.length) { thumbs.classList.add('hidden'); thumbs.classList.remove('flex'); return; }
        thumbs.classList.remove('hidden'); thumbs.classList.add('flex');
        files.forEach(function (f) {
          var d = document.createElement('div');
          d.className = 'relative h-20 w-20 overflow-hidden rounded-lg border border-line';
          var img = document.createElement('img');
          img.src = URL.createObjectURL(f);
          img.alt = '';
          img.className = 'h-full w-full object-cover';
          d.appendChild(img);
          thumbs.appendChild(d);
        });
      });

      /* ---- duplicate detection (advisory, non-blocking) ---- */
      function checkDup() {
        var hay = ($('f-name').value + ' ' + addressInput.value).toLowerCase();
        var hit = hay.indexOf('ana rosa') !== -1 || hay.indexOf('vila mariana') !== -1;
        $('dup-warning').classList.toggle('hidden', !hit);
        return hit;
      }
      $('f-name').addEventListener('input', checkDup);
      addressInput.addEventListener('input', function () { checkDup(); });

      /* ---- validation + submit ---- */
      var form = $('d1-form');
      function setErr(id, msg) {
        var e = $(id);
        if (!msg) { e.classList.add('hidden'); e.classList.remove('flex'); return true; }
        e.querySelector('span').textContent = msg;
        e.classList.remove('hidden'); e.classList.add('flex');
        return false;
      }
      function markInput(input, errId, msg) {
        var good = setErr(errId, msg);
        input.setAttribute('aria-invalid', String(!good));
        input.classList.toggle('border-danger', !good);
        input.classList.toggle('border-line', good);
        return { good: good, input: input };
      }
      function validate(requireAll) {
        var first = null;
        var r1 = markInput($('f-name'), 'e-name', (requireAll && !$('f-name').value.trim()) ? 'Give the spot a short, recognizable name.' : '');
        if (!r1.good) first = r1.input;
        var r2 = markInput(addressInput, 'e-address', (requireAll && !addressInput.value.trim()) ? 'Enter a street address so riders can find it.' : '');
        if (!r2.good && !first) first = r2.input;
        var pinOk = placed;
        setErr('e-pin', (requireAll && !pinOk) ? 'Place the pin on the map.' : '');
        var typePicked = !!document.querySelector('input[name=type]:checked');
        setErr('e-type', (requireAll && !typePicked) ? 'Pick a parking type.' : '');
        var paid = document.querySelector('input[name=cost][value=paid]').checked;
        var r3 = markInput($('f-amount'), 'e-amount', (paid && !$('f-amount').value) ? 'Enter the amount, or switch to Free.' : '');
        if (!r3.good && !first) first = r3.input;
        var ok = r1.good && r2.good && pinOk && typePicked && r3.good;
        return { ok: ok, first: first };
      }

      form.addEventListener('submit', function (e) {
        e.preventDefault();
        var v = validate(true);
        if (!v.ok) {
          if (v.first) v.first.focus();
          flash('Some fields need attention before publishing.');
          return;
        }
        var btn = $('submit-btn');
        btn.disabled = true;
        $('submit-label').textContent = 'Publishing…';
        setTimeout(function () {
          $('success-name').textContent = $('f-name').value.trim() || 'Your spot';
          form.classList.add('hidden');
          var s = $('d1-success');
          s.classList.remove('hidden');
          s.classList.add('block');
          window.scrollTo({ top: 0 });
        }, 900);
      });

      /* ---- state previews ---- */
      $('pv-dup').addEventListener('click', function () {
        $('f-name').value = 'Estação Vila Mariana — new entrance';
        checkDup();
        flash('Duplicate warning shown — it is advisory and does not block publishing.');
      });
      $('pv-error').addEventListener('click', function () {
        var v = validate(true);
        if (v.first) v.first.focus();
        flash('Validation errors shown — inline, with red borders (never color alone).');
      });
      $('pv-unverified').addEventListener('click', function () {
        flash('Unverified accounts see a block screen: "Verify your email to add parking locations."');
      });
    })();
  </script>
"""

write_page("d1-add-parking.html", "Add parking location",
           "Contribute a new bicycle parking location to the BikeNest community map.",
           breadcrumb([("Parking spots", "p2-search.html"), ("Add parking location", None)]),
           d1_main, "Prototype · D1 Add parking location", ICON_REGISTRY + d1_script)

# ================================================================= D2 — Propose change
SPOT = dict(
    name="Estação Vila Mariana",
    address="Domingos de Morais, 1500 — Vila Mariana, São Paulo",
    cost="R$ 5,00", unit="hour",
)

d2_changed_chip = f'<span class="changed-chip hidden items-center gap-1 rounded-full bg-[oklch(68%_0.12_75_/_0.14)] px-2.5 py-0.5 text-xs font-semibold text-[oklch(52%_0.12_70)]">{icon("pencil-line", "lucide h-3 w-3")} Changed</span>'

d2_pv_buttons = f"""
    <button type="button" id="pv-pending" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-3.5 py-2 text-xs font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("clock", "lucide h-3.5 w-3.5")} Proposal pending state</button>
    <button type="button" id="pv-diff" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-3.5 py-2 text-xs font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("git-pull-request", "lucide h-3.5 w-3.5")} Preview a sample change</button>
"""

d2_main = f"""
  <main id="content" data-od-id="d2-main" class="mx-auto w-full max-w-shell px-5 py-10 lg:px-8">
    <div class="mb-6 max-w-2xl">
      <p class="font-mono text-xs uppercase tracking-[0.14em] text-muted">Contribute · Propose change</p>
      <h1 class="mt-1 font-display text-3xl font-bold tracking-tight">Propose a change</h1>
      <p class="mt-2 text-[15px] leading-relaxed text-muted">You're editing <a href="p3-parking-details.html" class="font-medium text-accent-strong hover:text-accent-dark hover:underline">{SPOT['name']}</a>. Nothing changes on the live page until a moderator reviews your proposal — the current values stay visible in the history either way.</p>
    </div>

    <div class="mb-8 flex items-start gap-3 rounded-2xl border border-line bg-[oklch(56%_0.12_170_/_0.07)] p-4">
      <span class="mt-0.5 text-accent-strong">{icon("git-pull-request", "lucide h-5 w-5 shrink-0")}</span>
      <p class="text-sm leading-relaxed text-muted"><span class="font-medium text-fg">This is a proposal, not a direct edit.</span> Change only the fields you're confident about — each one is compared against the live value and reviewed individually.</p>
    </div>

    <div class="grid gap-8 lg:grid-cols-[minmax(0,1fr)_340px]">
      <!-- ===== Form column ===== -->
      <div>
        <form id="d2-form" novalidate data-dirty="0">
          {card("d2-location", "Location", "map-pin", d2_changed_chip.replace(' hidden', ''))}
            <div class="grid gap-5">
              <div>
                {field_label("f-name", "Name")}
                <input id="f-name" name="name" type="text" value="{SPOT['name']}"
                       class="mt-1.5 w-full rounded-xl border border-line bg-bg px-3.5 py-2.5 text-[15px] focus:border-accent-strong" />
                {err("e-name")}
              </div>
              <div>
                {field_label("f-address", "Street address")}
                <input id="f-address" name="address" type="text" value="{SPOT['address']}"
                       class="mt-1.5 w-full rounded-xl border border-line bg-bg px-3.5 py-2.5 text-[15px] focus:border-accent-strong" />
                {err("e-address")}
              </div>
            </div>
          {CLOSE}

          {card("d2-type-cost", "Type &amp; cost", "coins", d2_changed_chip)}
            <div class="grid gap-6">
              <fieldset>
                <legend class="text-sm font-medium">Parking type</legend>
                <div class="mt-2 grid grid-cols-2 gap-2 sm:grid-cols-4" role="radiogroup" aria-label="Parking type">
                  {TYPE_CHIPS}
                </div>
              </fieldset>
              <fieldset>
                <legend class="text-sm font-medium">Cost <span class="font-normal text-muted">· currently {SPOT['cost']} / hour</span></legend>
                <div class="mt-2 flex flex-wrap gap-2">
                  <label class="flex cursor-pointer items-center gap-2 rounded-xl border border-line px-3.5 py-2 text-sm font-medium transition-colors has-[:checked]:border-accent-strong has-[:checked]:bg-[oklch(56%_0.12_170_/_0.10)]">
                    <input type="radio" name="cost" value="free" class="accent-[oklch(45%_0.11_170)]" /> Free
                  </label>
                  <label class="flex cursor-pointer items-center gap-2 rounded-xl border border-line px-3.5 py-2 text-sm font-medium transition-colors has-[:checked]:border-accent-strong has-[:checked]:bg-[oklch(56%_0.12_170_/_0.10)]">
                    <input type="radio" name="cost" value="paid" class="accent-[oklch(45%_0.11_170)]" checked /> Paid
                  </label>
                </div>
                <div id="paid-fields" class="mt-3 grid gap-3 sm:grid-cols-[140px_1fr]">
                  <div>
                    {field_label("f-amount", "Amount")}
                    <div class="relative mt-1.5">
                      <span class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 font-mono text-sm text-muted">R$</span>
                      <input id="f-amount" type="number" min="0" step="0.5" value="5" class="w-full rounded-xl border border-line bg-bg py-2.5 pl-10 pr-3 text-[15px] focus:border-accent-strong" />
                    </div>
                  </div>
                  <div>
                    {field_label("f-unit", "Unit")}
                    <select id="f-unit" class="mt-1.5 w-full rounded-xl border border-line bg-bg px-3 py-2.5 text-[15px] focus:border-accent-strong">
                      <option value="hour" selected>per hour</option>
                      <option value="day">per day</option>
                      <option value="month">per month</option>
                    </select>
                  </div>
                </div>
              </fieldset>
            </div>
          {CLOSE}

          {card("d2-hours", "Opening hours", "clock", d2_changed_chip)}
            <p class="text-sm leading-relaxed text-muted">Hours shown are the live values. Adjust only what has actually changed — moderators review each field against the current entry.</p>
            <div id="hours-rows" class="mt-4 divide-y divide-line rounded-xl border border-line"><!-- rows injected by JS --></div>
          {CLOSE}

          {card("d2-security", "Security features", "shield-check", d2_changed_chip)}
            <div class="mt-4 flex flex-wrap gap-2">
              {SECURITY_CHIPS}
            </div>
          {CLOSE}

          {card("d2-description", "Description", "align-left", d2_changed_chip)}
            <div class="flex items-baseline justify-between">
              {field_label("f-desc", "Description")}
              <span id="desc-count" class="font-mono text-xs text-muted">0 / 600</span>
            </div>
            <textarea id="f-desc" rows="4" maxlength="600"
                      placeholder="Corrections or additions to the public description…"
                      class="mt-1.5 w-full resize-y rounded-xl border border-line bg-bg px-3.5 py-2.5 text-[15px] placeholder:text-[oklch(50%_0.018_240_/_0.55)] focus:border-accent-strong"></textarea>
          {CLOSE}

          <div class="mt-8 flex flex-col gap-4 border-t border-line pt-6 sm:flex-row sm:items-center sm:justify-between">
            <p class="text-xs leading-relaxed text-muted">Proposals are rate-limited. Your identity is recorded for moderation but never shown publicly.</p>
            <div class="flex shrink-0 items-center gap-3">
              <a href="p3-parking-details.html" class="rounded-xl px-4 py-2.5 text-sm font-medium text-muted hover:text-fg">Cancel</a>
              <button id="submit-btn" type="submit" class="inline-flex items-center gap-2 rounded-xl bg-accent-strong px-5 py-2.5 text-sm font-semibold text-white shadow-card transition-colors hover:bg-accent-dark disabled:cursor-wait disabled:opacity-70">
                {icon("send", "lucide h-4 w-4")}<span id="submit-label">Submit proposal</span>
              </button>
            </div>
          </div>
        </form>

        <!-- success state -->
        <section id="d2-success" class="hidden rounded-2xl border border-line bg-surface p-8 text-center shadow-card sm:p-12" aria-live="polite">
          <span class="mx-auto grid h-14 w-14 place-items-center rounded-full bg-[oklch(56%_0.12_170_/_0.12)] text-accent-strong">{icon("git-pull-request", "lucide h-7 w-7")}</span>
          <h2 class="mt-4 font-display text-2xl font-bold">Proposal submitted</h2>
          <p class="mx-auto mt-2 max-w-md text-[15px] leading-relaxed text-muted">A moderator will compare your changes against the live entry — approve, reject, or adjust them. You'll see the outcome in your <a href="c5-contributions.html" class="font-medium text-accent-strong hover:text-accent-dark hover:underline">contribution history</a>. The current values remain published and are preserved in the location's history.</p>
          <div class="mt-6 flex flex-wrap items-center justify-center gap-3">
            <a href="p3-parking-details.html" class="inline-flex items-center gap-2 rounded-xl bg-accent-strong px-5 py-2.5 text-sm font-semibold text-white shadow-card transition-colors hover:bg-accent-dark">Back to the spot {icon("arrow-right", "lucide h-4 w-4")}</a>
            <a href="c5-contributions.html" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-5 py-2.5 text-sm font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("clipboard-list", "lucide h-4 w-4")} Track the proposal</a>
          </div>
        </section>

        {STATE_PANEL.replace("{PV_BUTTONS}", d2_pv_buttons)}
      </div>

      <!-- ===== Rail ===== -->
      <aside class="lg:sticky lg:top-24 lg:self-start" aria-label="Change summary and history">
        <section data-od-id="d2-changes" class="rounded-2xl border border-line bg-surface p-5 shadow-card">
          <p class="font-mono text-[11px] uppercase tracking-wide text-muted">Your proposal changes</p>
          <p id="change-summary" class="mt-2 text-sm leading-relaxed text-muted">No changes yet — the form is pre-filled with the live values.</p>
          <ul id="change-list" class="mt-3 hidden grid gap-2.5"></ul>
        </section>

        <section data-od-id="d2-history" class="mt-4 rounded-2xl border border-line bg-surface p-5 shadow-card">
          <h2 class="flex items-center gap-2 font-display text-[15px] font-bold">{icon("history", "lucide h-4 w-4 text-muted")} Recent proposal history</h2>
          <ol class="mt-3 grid gap-3">
            <li class="flex gap-3">
              <span class="mt-1 h-2 w-2 shrink-0 rounded-full bg-[oklch(58%_0.13_155)]"></span>
              <div>
                <p class="text-sm leading-snug">Opening hours corrected — Sundays closed</p>
                <p class="mt-0.5 font-mono text-[11px] text-muted">Approved · Mar 12, 2026</p>
              </div>
            </li>
            <li class="flex gap-3">
              <span class="mt-1 h-2 w-2 shrink-0 rounded-full bg-[oklch(68%_0.12_75)]"></span>
              <div>
                <p class="text-sm leading-snug">Cost changed to R$ 6,00 / hour</p>
                <p class="mt-0.5 font-mono text-[11px] text-muted">Pending review · Mar 20, 2026</p>
              </div>
            </li>
          </ol>
          <p class="mt-4 border-t border-line pt-3 text-xs leading-relaxed text-muted">Important changes keep their history — old values stay auditable instead of being silently overwritten.</p>
        </section>
      </aside>
    </div>
  </main>
"""

d2_script = """
  <script>
    /* ---------- D2: propose change ---------- */
    (function () {
      var $ = function (id) { return document.getElementById(id); };

      /* ---- opening hours rows (live values pre-filled) ---- */
      var DAYS = ['Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday', 'Sunday'];
      var LIVE = [
        ['06:00', '22:00', true], ['06:00', '22:00', true], ['06:00', '22:00', true],
        ['06:00', '22:00', true], ['06:00', '22:00', true], ['08:00', '20:00', true],
        ['08:00', '18:00', false]
      ];
      var hoursBox = $('hours-rows');
      DAYS.forEach(function (day, i) {
        var d = LIVE[i];
        var row = document.createElement('div');
        row.className = 'flex flex-wrap items-center gap-x-3 gap-y-2 px-4 py-2.5';
        row.innerHTML =
          '<label class="flex w-28 shrink-0 items-center gap-2 text-sm">' +
          '<input type="checkbox" class="h-4 w-4 accent-[oklch(45%_0.11_170)]" ' + (d[2] ? 'checked' : '') + ' aria-label="' + day + ': open" />' + day + '</label>' +
          '<input type="time" value="' + d[0] + '" data-orig="' + d[0] + '" aria-label="' + day + ' opens at" class="w-28 rounded-lg border border-line bg-bg px-2.5 py-1.5 text-sm focus:border-accent-strong" />' +
          '<span class="text-xs text-muted">to</span>' +
          '<input type="time" value="' + d[1] + '" data-orig="' + d[1] + '" aria-label="' + day + ' closes at" class="w-28 rounded-lg border border-line bg-bg px-2.5 py-1.5 text-sm focus:border-accent-strong" />' +
          '<span class="hidden text-xs font-medium text-muted">Closed</span>';
        hoursBox.appendChild(row);
        var cb = row.querySelector('input[type=checkbox]');
        var times = row.querySelectorAll('input[type=time]');
        var closedTag = row.querySelector('span.hidden');
        function sync() {
          var open = cb.checked;
          times.forEach(function (t) { t.disabled = !open; t.classList.toggle('opacity-40', !open); });
          closedTag.classList.toggle('hidden', open);
        }
        cb.addEventListener('change', sync);
        sync();
      });

      /* ---- cost toggle ---- */
      document.querySelectorAll('input[name=cost]').forEach(function (r) {
        r.addEventListener('change', function () {
          var paid = document.querySelector('input[name=cost][value=paid]').checked;
          $('paid-fields').classList.toggle('hidden', !paid);
        });
      });

      /* ---- description counter ---- */
      var desc = $('f-desc');
      desc.addEventListener('input', function () { $('desc-count').textContent = desc.value.length + ' / 600'; });

      /* ---- change detection ---- */
      var ORIG = {
        name: $('f-name').value,
        address: $('f-address').value,
        type: 'covered',
        amount: '5',
        unit: 'hour',
        cost: 'paid',
        sec: ['staffed', 'cctv', 'covered', 'lighting'],
        desc: ''
      };
      function humanDay(v) { return v === 'free' ? 'Free' : ('R$ ' + $('f-amount').value + ' / ' + $('f-unit').value); }
      function collectChanges() {
        var out = [];
        if ($('f-name').value !== ORIG.name) out.push(['Name', ORIG.name, $('f-name').value]);
        if ($('f-address').value !== ORIG.address) out.push(['Address', ORIG.address, $('f-address').value]);
        var t = document.querySelector('input[name=type]:checked');
        if (t && t.value !== ORIG.type) out.push(['Type', 'Covered', TYPE_LABEL[t.value]]);
        var cost = document.querySelector('input[name=cost]:checked').value;
        if (cost !== ORIG.cost || $('f-amount').value !== ORIG.amount || $('f-unit').value !== ORIG.unit) {
          out.push(['Cost', humanDay(ORIG.cost), humanDay(cost)]);
        }
        var secNow = Array.prototype.map.call(document.querySelectorAll('.sec-opt input:checked'), function (i) { return i.value; });
        if (secNow.join() !== ORIG.sec.join()) out.push(['Security', ORIG.sec.length + ' features', secNow.length + ' features']);
        if (desc.value.trim() !== ORIG.desc) out.push(['Description', '—', 'updated']);
        document.querySelectorAll('#hours-rows input[type=time]').forEach(function (t) {
          if (t.value !== t.getAttribute('data-orig')) {
            var day = t.closest('div').querySelector('label').textContent.trim();
            out.push(['Hours — ' + day, t.getAttribute('data-orig'), t.value]);
          }
        });
        return out;
      }
      var TYPE_LABEL = { rack: 'Rack', covered: 'Covered', staffed: 'Staffed', lockers: 'Lockers' };
      function refreshChanges() {
        var changes = collectChanges();
        document.querySelectorAll('[data-od-id] .changed-chip').forEach(function (chip) { chip.classList.add('hidden'); chip.classList.remove('flex'); });
        changes.forEach(function (c) {
          var sectionId = { Name: 'd2-location', Address: 'd2-location', Type: 'd2-type-cost', Cost: 'd2-type-cost', Description: 'd2-description' }[c[0]];
          if (c[0].indexOf('Hours') === 0) sectionId = 'd2-hours';
          if (['staffed', 'cctv', 'covered', 'lockers', 'controlled', 'lighting'].indexOf(c[0]) !== -1) sectionId = 'd2-security';
          if (sectionId) {
            var chip = document.querySelector('[data-od-id="' + sectionId + '"] .changed-chip');
            if (chip) { chip.classList.remove('hidden'); chip.classList.add('flex'); }
          }
        });
        var list = $('change-list');
        list.innerHTML = '';
        if (!changes.length) {
          $('change-summary').textContent = 'No changes yet — the form is pre-filled with the live values.';
          list.classList.add('hidden'); list.classList.remove('flex');
          return;
        }
        $('change-summary').textContent = changes.length + (changes.length === 1 ? ' field changed.' : ' fields changed.');
        list.classList.remove('hidden'); list.classList.add('flex');
        changes.forEach(function (c) {
          var li = document.createElement('li');
          li.className = 'rounded-lg bg-[oklch(96%_0.006_240)] px-3 py-2';
          li.innerHTML = '<p class="text-[11px] font-semibold uppercase tracking-wide text-muted">' + c[0] + '</p>' +
            '<p class="mt-0.5 text-sm leading-snug"><span class="text-muted line-through">' + escapeHtml(String(c[1]).slice(0, 42)) + '</span> → <span class="font-medium">' + escapeHtml(String(c[2]).slice(0, 42)) + '</span></p>';
          list.appendChild(li);
        });
      }
      function escapeHtml(s) { var d = document.createElement('div'); d.textContent = s; return d.innerHTML; }
      document.querySelectorAll('#d2-form input, #d2-form textarea, #d2-form select').forEach(function (el) {
        el.addEventListener('change', refreshChanges);
        el.addEventListener('input', refreshChanges);
      });
      refreshChanges();

      /* ---- security chip "changed" section id ---- */
      document.querySelectorAll('.sec-opt input').forEach(function (cb) {
        cb.dataset.section = 'd2-security';
      });

      /* ---- validation + submit ---- */
      var form = $('d2-form');
      function setErr(id, msg) {
        var e = $(id);
        if (!msg) { e.classList.add('hidden'); e.classList.remove('flex'); return true; }
        e.querySelector('span').textContent = msg;
        e.classList.remove('hidden'); e.classList.add('flex');
        return false;
      }
      form.addEventListener('submit', function (e) {
        e.preventDefault();
        var ok = true, first = null;
        [['f-name', 'e-name'], ['f-address', 'e-address']].forEach(function (pair) {
          var input = $(pair[0]);
          var good = setErr(pair[1], input.value.trim() ? '' : 'This field can\u2019t be empty in a proposal.');
          input.setAttribute('aria-invalid', String(!good));
          input.classList.toggle('border-danger', !good);
          input.classList.toggle('border-line', good);
          if (!good && !first) first = input;
          ok = ok && good;
        });
        if (!collectChanges().length) {
          flash('Nothing to propose yet — change at least one field first.');
          return;
        }
        if (!ok) { if (first) first.focus(); return; }
        var btn = $('submit-btn');
        btn.disabled = true;
        $('submit-label').textContent = 'Submitting…';
        setTimeout(function () {
          form.classList.add('hidden');
          var s = $('d2-success');
          s.classList.remove('hidden');
          s.classList.add('block');
          window.scrollTo({ top: 0 });
        }, 900);
      });

      /* ---- state previews ---- */
      $('pv-pending').addEventListener('click', function () {
        flash('After submit, the proposal shows "Pending review" in your contribution history until a moderator decides.');
      });
      $('pv-diff').addEventListener('click', function () {
        $('f-amount').value = '6';
        refreshChanges();
        flash('Sample change applied — the rail now shows the old value crossed out next to yours.');
      });
    })();
  </script>
"""

write_page("d2-propose-change.html", "Propose a change",
           "Propose corrections to an existing bicycle parking entry — reviewed by moderators before going live.",
           breadcrumb([("Parking spots", "p2-search.html"), (SPOT['name'], "p3-parking-details.html"), ("Propose change", None)]),
           d2_main, "Prototype · D2 Propose change", ICON_REGISTRY + d2_script)

# ================================================================= D3 — Write / edit review
d3_pv_buttons = f"""
    <button type="button" id="pv-new" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-3.5 py-2 text-xs font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("pencil-line", "lucide h-3.5 w-3.5")} New-review mode</button>
    <button type="button" id="pv-pending" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-3.5 py-2 text-xs font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("clock", "lucide h-3.5 w-3.5")} Pending moderation state</button>
"""

d3_main = f"""
  <main id="content" data-od-id="d3-main" class="mx-auto w-full max-w-2xl px-5 py-10 lg:px-8">
    <div class="mb-8">
      <p class="font-mono text-xs uppercase tracking-[0.14em] text-muted">Contribute · Review</p>
      <h1 class="mt-1 font-display text-3xl font-bold tracking-tight" id="page-title">Edit your review</h1>
      <p class="mt-2 text-[15px] leading-relaxed text-muted">Reviews help the next rider judge a spot in seconds. Be specific and fair — and remember your review is public.</p>
    </div>

    <!-- Spot context card -->
    <section data-od-id="d3-context" class="mb-6 flex items-center gap-4 rounded-2xl border border-line bg-surface p-4 shadow-card">
      <span class="grid h-12 w-12 shrink-0 place-items-center rounded-xl bg-[oklch(56%_0.12_170_/_0.10)] text-accent-strong">{icon("user-check", "lucide h-5 w-5")}</span>
      <div class="min-w-0 flex-1">
        <h2 class="truncate font-display text-[15px] font-bold">{SPOT['name']}</h2>
        <p class="mt-0.5 flex items-center gap-1 text-xs text-muted">{icon("map-pin", "lucide h-3 w-3 shrink-0")}<span class="truncate">{SPOT['address']}</span></p>
      </div>
      <div class="hidden shrink-0 text-right sm:block">
        <p class="flex items-center justify-end gap-1 font-display text-lg font-bold">{icon("star", "lucide h-4 w-4 fill-current")} 4.8</p>
        <p class="font-mono text-[11px] text-muted">20 ratings</p>
      </div>
    </section>

    <!-- Edit notice -->
    <div id="edit-notice" class="mb-6 flex items-start gap-3 rounded-2xl border border-line bg-[oklch(56%_0.12_170_/_0.07)] p-4">
      <span class="mt-0.5 text-accent-strong">{icon("history", "lucide h-4 w-4 shrink-0")}</span>
      <p class="text-sm leading-relaxed text-muted">You reviewed this spot on <span class="font-medium text-fg">March 2, 2026</span> — you're editing that review. One active review per spot; your previous version is preserved in the edit history.</p>
    </div>

    <form id="d3-form" novalidate>
      <section data-od-id="d3-rating" class="rounded-2xl border border-line bg-surface p-6 shadow-card sm:p-7">
        <div class="flex flex-wrap items-center gap-2.5">
          <span class="grid h-8 w-8 place-items-center rounded-xl bg-[oklch(56%_0.12_170_/_0.10)] text-accent-strong">{icon("star", "lucide h-4 w-4")}</span>
          <h2 class="font-display text-lg font-bold">Your rating</h2>
          <span id="rating-label" class="ml-auto text-sm font-medium text-muted">Good</span>
        </div>
        <div id="star-group" role="radiogroup" aria-label="Rating from 1 to 5 stars" class="mt-4 flex gap-1.5">
          <!-- stars injected by JS -->
        </div>
        {err("e-rating")}
        <p class="mt-3 text-xs text-muted">1 = would not recommend · 5 = excellent</p>
      </section>

      <section data-od-id="d3-text" class="mt-4 rounded-2xl border border-line bg-surface p-6 shadow-card sm:p-7">
        <div class="flex flex-wrap items-center gap-2.5">
          <span class="grid h-8 w-8 place-items-center rounded-xl bg-[oklch(56%_0.12_170_/_0.10)] text-accent-strong">{icon("align-left", "lucide h-4 w-4")}</span>
          <h2 class="font-display text-lg font-bold">Your review</h2>
          <span id="text-count" class="ml-auto font-mono text-xs text-muted">0 / 500</span>
        </div>
        <textarea id="f-review" rows="5" minlength="20" maxlength="500"
                  placeholder="What should the next rider judge? How busy does it get, how safe does it feel, is it easy to find…"
                  class="mt-4 w-full resize-y rounded-xl border border-line bg-bg px-3.5 py-2.5 text-[15px] leading-relaxed placeholder:text-[oklch(50%_0.018_240_/_0.55)] focus:border-accent-strong">Staffed all day and the attendant keeps an eye on the bikes. The entrance is a bit hidden behind the ticket gates — follow the signs for "bicicletário". Two weekends in a row, spots were still free at 10am.</textarea>
        {err("e-review")}
      </section>

      <section data-od-id="d3-photos" class="mt-6 rounded-2xl border border-line bg-surface p-6 shadow-card sm:p-7">
        <div class="flex flex-wrap items-center gap-2.5">
          <span class="grid h-8 w-8 place-items-center rounded-xl bg-[oklch(56%_0.12_170_/_0.10)] text-accent-strong">{icon("camera", "lucide h-4 w-4")}</span>
          <h2 class="font-display text-lg font-bold">Photos <span class="font-normal text-sm text-muted">· optional</span></h2>
        </div>
        <p class="mt-3 text-sm text-muted">Photos go through moderation before publishing. Location metadata is removed automatically.</p>
        <label id="dropzone" class="mt-3 flex cursor-pointer flex-col items-center justify-center gap-2 rounded-xl border-2 border-dashed border-line bg-[oklch(98%_0.004_240)] px-6 py-7 text-center transition-colors hover:border-accent-strong hover:bg-[oklch(56%_0.12_170_/_0.06)]">
          {icon("image-plus", "lucide h-6 w-6 text-muted")}
          <span class="text-sm font-medium">Add photos to your review</span>
          <input id="f-photos" type="file" accept="image/*" multiple class="sr-only" />
        </label>
        <div id="thumbs" class="mt-3 hidden flex-wrap gap-3"></div>
      </section>

      <div class="mt-8 flex flex-col gap-4 border-t border-line pt-6 sm:flex-row sm:items-center sm:justify-between">
        <p class="text-xs leading-relaxed text-muted">Reviews are rate-limited and pass a moderation check before appearing publicly.<br />Edits preserve audit history — nothing is silently overwritten.</p>
        <div class="flex shrink-0 items-center gap-3">
          <a href="p3-parking-details.html" class="rounded-xl px-4 py-2.5 text-sm font-medium text-muted hover:text-fg">Cancel</a>
          <button id="submit-btn" type="submit" class="inline-flex items-center gap-2 rounded-xl bg-accent-strong px-5 py-2.5 text-sm font-semibold text-white shadow-card transition-colors hover:bg-accent-dark disabled:cursor-wait disabled:opacity-70">
            {icon("send", "lucide h-4 w-4")}<span id="submit-label">Save changes</span>
          </button>
        </div>
      </div>
    </form>

    <!-- success state -->
    <section id="d3-success" class="hidden rounded-2xl border border-line bg-surface p-8 text-center shadow-card sm:p-12" aria-live="polite">
      <span class="mx-auto grid h-14 w-14 place-items-center rounded-full bg-[oklch(58%_0.13_155_/_0.12)] text-fresh">{icon("check-circle-2", "lucide h-7 w-7")}</span>
      <h2 id="success-title" class="mt-4 font-display text-2xl font-bold">Review updated</h2>
      <p id="success-copy" class="mx-auto mt-2 max-w-md text-[15px] leading-relaxed text-muted">Your review is live on the spot's page. The previous version stays in the audit history, and the spot's rating has been recalculated.</p>
      <div class="mt-6 flex flex-wrap items-center justify-center gap-3">
        <a href="p3-parking-details.html" class="inline-flex items-center gap-2 rounded-xl bg-accent-strong px-5 py-2.5 text-sm font-semibold text-white shadow-card transition-colors hover:bg-accent-dark">See it on the spot's page {icon("arrow-right", "lucide h-4 w-4")}</a>
        <a href="c5-contributions.html" class="inline-flex items-center gap-2 rounded-xl border border-line bg-surface px-5 py-2.5 text-sm font-medium hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("clipboard-list", "lucide h-4 w-4")} Contribution history</a>
      </div>
    </section>

    {STATE_PANEL.replace("{PV_BUTTONS}", d3_pv_buttons)}
  </main>
"""

d3_script = """
  <script>
    /* ---------- D3: write / edit review ---------- */
    (function () {
      var $ = function (id) { return document.getElementById(id); };
      var RATING_WORDS = ['—', 'Poor', 'Fair', 'Good', 'Very good', 'Excellent'];

      /* ---- star rating input ---- */
      var starGroup = $('star-group') || document.getElementById('star-group');
      var group = document.querySelector('[role=radiogroup][aria-label^="Rating"]');
      var current = 4;
      var stars = [];
      for (var i = 1; i <= 5; i++) {
        var b = document.createElement('button');
        b.type = 'button';
        b.dataset.value = i;
        b.setAttribute('role', 'radio');
        b.setAttribute('aria-checked', String(i === current));
        b.setAttribute('aria-label', i + ' star' + (i > 1 ? 's' : '') + ' — ' + RATING_WORDS[i]);
        b.className = 'star-btn grid h-11 w-11 place-items-center rounded-lg text-muted transition-colors hover:bg-[oklch(20%_0.02_240_/_0.05)]';
        stars.push(b);
        group.appendChild(b);
      }
      function starSvg(filled) {
        var cls = filled ? 'lucide h-6 w-6 fill-[oklch(68%_0.12_75)] text-[oklch(68%_0.12_75)]' : 'lucide h-6 w-6';
        return '<svg aria-hidden="true" class="' + cls + '" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 11.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/></svg>';
      }
      function paint(hoverVal) {
        var v = hoverVal || current;
        stars.forEach(function (b) {
          var val = Number(b.dataset.value);
          b.innerHTML = starSvg(val <= v);
          b.classList.toggle('text-[oklch(68%_0.12_75)]', val <= v);
          b.classList.toggle('text-muted', val > v);
        });
        $('rating-label').textContent = RATING_WORDS[v];
        $('rating-label').classList.toggle('text-muted', !hoverVal);
        $('rating-label').classList.toggle('text-fg', !!hoverVal || v > 0);
        stars.forEach(function (b) { b.setAttribute('aria-checked', String(Number(b.dataset.value) === current)); });
      }
      stars.forEach(function (b) {
        b.addEventListener('click', function () { current = Number(b.dataset.value); paint(); clearRatingError(); });
        b.addEventListener('mouseenter', function () { paint(Number(b.dataset.value)); });
        b.addEventListener('mouseleave', function () { paint(); });
      });
      group.addEventListener('keydown', function (e) {
        if (e.key === 'ArrowRight' || e.key === 'ArrowUp') { current = Math.min(5, current + 1); paint(); clearRatingError(); e.preventDefault(); }
        if (e.key === 'ArrowLeft' || e.key === 'ArrowDown') { current = Math.max(1, current - 1); paint(); clearRatingError(); e.preventDefault(); }
      });
      paint();

      /* ---- text counter ---- */
      var review = $('f-review');
      review.addEventListener('input', function () {
        $('text-count').textContent = review.value.length + ' / 500';
      });
      $('text-count').textContent = review.value.length + ' / 500';

      /* ---- photos ---- */
      $('f-photos').addEventListener('change', function (e) {
        var files = Array.prototype.slice.call(e.target.files).slice(0, 4);
        var thumbs = $('thumbs');
        thumbs.innerHTML = '';
        if (!files.length) { thumbs.classList.add('hidden'); thumbs.classList.remove('flex'); return; }
        thumbs.classList.remove('hidden'); thumbs.classList.add('flex');
        files.forEach(function (f) {
          var d = document.createElement('div');
          d.className = 'relative h-20 w-20 overflow-hidden rounded-lg border border-line';
          var img = document.createElement('img');
          img.src = URL.createObjectURL(f);
          img.alt = '';
          img.className = 'h-full w-full object-cover';
          d.appendChild(img);
          thumbs.appendChild(d);
        });
      });

      /* ---- validation + submit ---- */
      var form = $('d3-form');
      function setRatingError(msg) {
        var e = $('e-review');
        if (!msg) { e.classList.add('hidden'); e.classList.remove('flex'); return; }
        e.querySelector('span').textContent = msg;
        e.classList.remove('hidden'); e.classList.add('flex');
      }
      function clearRatingError() { setRatingError(''); }
      form.addEventListener('submit', function (e) {
        e.preventDefault();
        var bad = false;
        if (current < 1) { setRatingError('Pick a star rating before publishing.'); bad = true; }
        if (review.value.trim().length < 20) {
          review.setAttribute('aria-invalid', 'true');
          review.classList.add('border-danger');
          review.classList.remove('border-line');
          flash('Reviews need at least 20 characters — add a bit more detail.');
          bad = true;
        } else {
          review.setAttribute('aria-invalid', 'false');
          review.classList.remove('border-danger');
          review.classList.add('border-line');
        }
        if (bad) return;
        var btn = $('submit-btn');
        btn.disabled = true;
        $('submit-label').textContent = 'Saving…';
        setTimeout(function () {
          form.classList.add('hidden');
          $('edit-notice').classList.add('hidden');
          var s = $('d3-success');
          s.classList.remove('hidden');
          s.classList.add('block');
          window.scrollTo({ top: 0 });
        }, 900);
      });

      /* ---- state previews ---- */
      $('pv-new').addEventListener('click', function () {
        document.getElementById('page-title').textContent = 'Write a review';
        $('submit-label').textContent = 'Publish review';
        $('edit-notice').classList.add('hidden');
        current = 0; paint();
        review.value = '';
        review.dispatchEvent(new Event('input'));
        flash('New-review mode: empty rating and text, publish button.');
      });
      $('pv-pending').addEventListener('click', function () {
        $('success-title').textContent = 'Review submitted';
        $('success-copy').textContent = 'Your review is pending a moderation check and will appear on the spot\u2019s page shortly. You can track its status in your contribution history.';
        form.classList.add('hidden');
        var s = $('d3-success');
        s.classList.remove('hidden');
        s.classList.add('block');
        window.scrollTo({ top: 0 });
      });
    })();
  </script>
"""

write_page("d3-write-review.html", "Write or edit a review",
           "Rate and review a bicycle parking spot — one active review per rider, editable anytime.",
           breadcrumb([("Parking spots", "p2-search.html"), (SPOT['name'], "p3-parking-details.html"), ("Write review", None)]),
           d3_main, "Prototype · D3 Write / edit review", ICON_REGISTRY + d3_script)
