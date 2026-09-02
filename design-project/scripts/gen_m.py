#!/usr/bin/env python3
# Generates M1-M4 moderation screens with a shared moderator shell.
import re, os

ICON_DIR = 'assets/vendor/icons'
_cache = {}
def icon(name, cls='h-4 w-4'):
    if name not in _cache:
        s = open(os.path.join(ICON_DIR, name + '.svg')).read()
        s = ' '.join(s.split())
        s = re.sub(r'class="lucide[^"]*"', '', s, count=1)
        s = s.replace('<svg ', '<svg aria-hidden="true" ', 1)
        _cache[name] = s
    s = _cache[name]
    return s.replace('<svg ', f'<svg class="{cls}" ', 1)

# ---------------- Shared shell ----------------
TOKENS = open('c1-account.html').read()
m = re.search(r'<script>\s*(/\* Design tokens.*?)</script>', TOKENS, re.S)
TOKENS_JS = m.group(1)

def header(active):
    def mi(href, ic, label, cur):
        cls = 'font-medium text-fg' if cur else 'text-fg'
        return (f'<a role="menuitem" href="{href}" class="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm {cls} '
                'hover:bg-[oklch(20%_0.02_240_/_0.05)]">' + icon(ic, 'h-4 w-4 text-muted') + f' {label}</a>')
    mob = lambda href, label, cur: (f'<a href="{href}" class="rounded-lg px-3 py-2.5 text-[15px] '
        + ('font-medium text-fg' if cur else 'text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg')
        + f'">{label}</a>')
    mod_links = ''.join(mi(h, i, l, h == active) for h, i, l in [
        ('m1-moderation.html', 'shield', 'Moderation overview'),
        ('m2-photos.html', 'image', 'Photo queue'),
        ('m3-reports.html', 'flag', 'Reports queue'),
        ('m4-proposals.html', 'git-pull-request', 'Proposal review')])
    mod_mob = ''.join(mob(h, l, h == active) for h, l in [
        ('m1-moderation.html', 'Moderation overview'),
        ('m2-photos.html', 'Photo queue'),
        ('m3-reports.html', 'Reports queue'),
        ('m4-proposals.html', 'Proposal review')])
    return f'''
  <header data-od-id="topnav-moderation" class="sticky top-0 z-40 border-b border-line bg-[oklch(98%_0.004_240_/_0.9)] backdrop-blur">
    <div class="mx-auto flex h-16 max-w-shell items-center justify-between gap-6 px-5 lg:px-8">
      <a href="p1-landing.html" class="flex items-center gap-2.5" aria-label="BikeNest — home">
        <span class="grid h-9 w-9 place-items-center rounded-xl bg-accent text-white">{icon('bike', 'h-5 w-5')}</span>
        <span class="font-display text-lg font-bold tracking-tight">BikeNest</span>
      </a>
      <nav class="hidden items-center gap-7 md:flex" aria-label="Primary">
        <a href="p2-search.html" class="text-sm text-muted hover:text-fg">Parking spots</a>
        <a href="p1-landing.html#how-it-works" class="text-sm text-muted hover:text-fg">How it works</a>
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
            {icon('chevron-down', 'h-4 w-4 text-muted')}
          </button>
          <div id="user-menu" role="menu" aria-label="Account menu"
               class="hidden absolute right-0 top-[calc(100%+8px)] w-64 rounded-xl border border-line bg-surface p-1.5 shadow-pop">
            <div class="border-b border-line px-3 py-2.5">
              <p class="flex items-center gap-2 text-sm font-semibold">Ana Ribeiro
                <span class="inline-flex items-center gap-1 rounded-full bg-[oklch(56%_0.12_170_/_0.12)] px-2 py-0.5 text-[11px] font-semibold text-accent-strong">{icon('shield', 'h-3 w-3')} Moderator</span>
              </p>
              <p class="mt-0.5 truncate text-xs text-muted">ana.ribeiro@example.com</p>
            </div>
            <div class="mt-1">
              {mi('c1-account.html', 'user', 'Account overview', False)}
              {mi('c5-contributions.html', 'clipboard-list', 'Contributions', False)}
              {mi('c6-privacy.html', 'shield-check', 'Privacy &amp; data', False)}
            </div>
            <div class="my-1 border-t border-line"></div>
            <p class="px-3 pb-1 pt-1 font-mono text-[11px] uppercase tracking-wide text-muted">Moderation</p>
            {mod_links}
            <div class="my-1 border-t border-line"></div>
            <a role="menuitem" href="a2-login.html" class="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm font-medium text-danger hover:bg-danger-soft">{icon('log-out')} Log out</a>
          </div>
        </div>
      </div>
      <button id="menu-btn" class="grid h-10 w-10 place-items-center rounded-lg border border-line md:hidden" aria-expanded="false" aria-controls="mobile-menu" aria-label="Open menu">
        {icon('menu', 'h-5 w-5')}
      </button>
    </div>
    <div id="mobile-menu" class="hidden border-t border-line bg-bg px-5 py-4 md:hidden">
      <nav class="flex flex-col gap-1" aria-label="Primary mobile">
        <a href="p2-search.html" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Parking spots</a>
        <a href="p1-landing.html#how-it-works" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">How it works</a>
      </nav>
      <div class="mt-3 flex flex-col gap-1 border-t border-line pt-3">
        <p class="px-3 pb-1 font-mono text-[11px] uppercase tracking-wide text-muted">Moderation</p>
        {mod_mob}
      </div>
      <div class="mt-3 flex flex-col gap-1 border-t border-line pt-3">
        <p class="px-3 pb-1 font-mono text-[11px] uppercase tracking-wide text-muted">Your account</p>
        <a href="c1-account.html" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Account overview</a>
        <a href="c5-contributions.html" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Contributions</a>
        <a href="a2-login.html" class="mt-1 rounded-lg px-3 py-2.5 text-[15px] font-medium text-danger hover:bg-danger-soft">Log out</a>
      </div>
    </div>
  </header>'''

def tabs(active):
    def t(href, label):
        cur = ' border-accent-strong font-medium text-fg' if active == href else ' border-transparent text-muted hover:border-line hover:text-fg'
        return f'<a href="{href}"{" aria-current=\"page\"" if active == href else ""} class="whitespace-nowrap border-b-2{cur} py-3.5 text-sm">{label}</a>'
    return ('<nav data-od-id="moderation-tabs" aria-label="Moderation sections" class="border-b border-line bg-bg">'
            '<div class="mx-auto flex max-w-shell gap-6 overflow-x-auto px-5 lg:px-8">'
            + t('m1-moderation.html', 'Overview') + t('m2-photos.html', 'Photos')
            + t('m3-reports.html', 'Reports') + t('m4-proposals.html', 'Proposals')
            + '</div></nav>')

def footer(tag):
    return f'''
  <footer data-od-id="footer" class="mt-4 border-t border-line bg-surface">
    <div class="mx-auto max-w-shell px-5 py-12 lg:px-8">
      <div class="flex flex-col gap-10 md:flex-row md:items-start md:justify-between">
        <div class="max-w-xs">
          <a href="p1-landing.html" class="flex items-center gap-2.5" aria-label="BikeNest — home">
            <span class="grid h-8 w-8 place-items-center rounded-lg bg-accent text-white">{icon('bike', 'h-4 w-4')}</span>
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
        <p class="font-mono text-xs text-muted">Prototype · {tag}</p>
      </div>
    </div>
  </footer>
  <div class="pointer-events-none fixed inset-x-0 bottom-6 z-50 flex justify-center px-5">
    <p id="flash" role="status" aria-live="polite" class="pointer-events-auto hidden max-w-md items-center gap-2.5 rounded-xl bg-fg px-4 py-3 text-sm text-bg shadow-pop">
      {icon('info', 'h-4 w-4 shrink-0')}
      <span id="flash-text"></span>
    </p>
  </div>
  <script>
    {TOKENS_JS.strip()}
  </script>'''

def shell_js():
    s = open('c1-account.html').read()
    start = s.index('/* ---------- Shell')
    return s[start:s.index('  </script>', start)]

def page(fname, title, desc, tag, active, main, extra_js):
    shell = shell_js()
    html = f'''<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{title} — BikeNest</title>
  <meta name="description" content="{desc}" />
  <meta name="robots" content="noindex" />
  <script src="https://cdn.tailwindcss.com"></script>
  <script>
    {TOKENS_JS.strip()}
  </script>
  <style type="text/tailwindcss">
    @layer base {{
      html {{ -webkit-text-size-adjust: 100%; }}
      body {{ text-rendering: optimizeLegibility; -webkit-font-smoothing: antialiased; }}
      p {{ text-wrap: pretty; }}
      h1, h2, h3 {{ text-wrap: balance; }}
      :focus-visible {{ outline: 2px solid oklch(45% 0.11 170); outline-offset: 2px; border-radius: 4px; }}
    }}
  </style>
</head>
<body class="bg-bg font-body text-fg">

{header(active)}
{tabs(active)}

  <main id="content" class="mx-auto w-full max-w-6xl px-5 py-10 lg:px-8">
{main}
  </main>
{footer(tag)}

  <script>
{shell}{extra_js}
  </script>

</body>
</html>
'''
    open(fname, 'w').write(html)
    print('wrote', fname, len(html.splitlines()), 'lines')

# ---------------- Shared partials ----------------
def state_chip(state):
    styles = {
        'open': ('border-[oklch(68%_0.12_75_/_0.45)] bg-[oklch(68%_0.12_75_/_0.12)] text-aging', 'Open'),
        'review': ('bg-[oklch(56%_0.12_170_/_0.12)] text-accent-strong', 'Under review'),
        'resolved': ('bg-[oklch(58%_0.13_155_/_0.12)] text-fresh', 'Resolved'),
        'dismissed': ('bg-[oklch(20%_0.02_240_/_0.06)] text-muted', 'Dismissed'),
    }[state]
    return f'<span class="inline-flex items-center gap-1.5 rounded-full border border-line px-2.5 py-1 text-xs font-medium {styles[0]}">{styles[1]}</span>'

AUDIT_NOTE = ('Every moderation action is recorded in an audit trail with the actor, target and timestamp. '
              'Audit history cannot be edited or deleted — corrections appear as new entries (§44).')

# ================= M1 — Moderation dashboard =================
def queue_card(href, ic, count, label, note, extra=''):
    return f'''
        <a href="{href}" data-od-id="queue-card-{label.split()[0].lower()}" class="group flex flex-col rounded-2xl border border-line bg-surface p-6 shadow-card transition-all hover:shadow-pop">
          <div class="flex items-center justify-between">
            <span class="grid h-11 w-11 place-items-center rounded-xl bg-[oklch(56%_0.12_170_/_0.10)] text-accent-strong">{icon(ic, 'h-5 w-5')}</span>
            <span class="font-display text-4xl font-bold tracking-tight">{count}</span>
          </div>
          <h3 class="mt-4 font-display text-lg font-bold">{label}</h3>
          <p class="mt-1 flex-1 text-sm leading-relaxed text-muted">{note}</p>
          <span class="mt-4 inline-flex items-center gap-1.5 text-sm font-medium text-accent-strong">Open queue {icon('arrow-right', 'h-4 w-4 transition-transform group-hover:translate-x-0.5')}</span>
        </a>'''

activity = [
    ('check', 'fresh', 'Photo approved', 'Bicicletário Metrô Ana Rosa · submitted by contributor #0917', '2 h ago'),
    ('flag', 'danger', 'Report resolved', 'Incorrect price on Estação Vila Mariana — cost corrected to R$ 4/hour', 'Yesterday'),
    ('git-pull-request', 'accent', 'Proposal approved', 'Opening-hours change on Estação Paulista applied', 'Yesterday'),
    ('x', 'danger', 'Photo rejected', "Doesn't show the parking — Rua Augusta hub · contributor notified", '2 d ago'),
    ('user-x', 'muted', 'Warning issued', 'Report spam — contributor #0871 reminded of the acceptable-use policy', '3 d ago'),
]
feed = ''
for ic, tone, title, detail, when in activity:
    tone_bg = {'fresh': 'bg-[oklch(58%_0.13_155_/_0.10)] text-fresh', 'danger': 'bg-danger-soft text-danger',
               'muted': 'bg-[oklch(20%_0.02_240_/_0.05)] text-muted',
               'accent': 'bg-[oklch(56%_0.12_170_/_0.10)] text-accent-strong'}[tone]
    feed += f'''
        <li class="flex items-center justify-between gap-4 py-3.5">
          <div class="flex items-center gap-3">
            <span class="grid h-9 w-9 shrink-0 place-items-center rounded-full {tone_bg}">{icon(ic, 'h-4 w-4')}</span>
            <div>
              <p class="text-sm font-medium">{title}</p>
              <p class="mt-0.5 text-xs text-muted">{detail}</p>
            </div>
          </div>
          <time class="shrink-0 font-mono text-xs text-muted">{when}</time>
        </li>'''

M1_MAIN = f'''    <div class="mb-8 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
      <div>
        <p class="font-mono text-xs uppercase tracking-[0.14em] text-muted">Moderation</p>
        <h1 class="mt-1 font-display text-3xl font-bold tracking-tight">Overview</h1>
        <p class="mt-2 max-w-2xl text-sm leading-relaxed text-muted">Queues waiting for review. Content stays hidden or flagged until a decision is recorded here.</p>
      </div>
      <span class="inline-flex w-fit items-center gap-1.5 rounded-full bg-[oklch(56%_0.12_170_/_0.12)] px-3 py-1.5 text-xs font-semibold text-accent-strong">{icon('shield-check', 'h-3.5 w-3.5')} Signed in as Moderator</span>
    </div>

    <div class="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">{queue_card('m2-photos.html', 'image', 5, 'Photos pending review', 'Uploaded photos stay invisible to riders until approved. Oldest waiting: 5 days.')}
    {queue_card('m3-reports.html', 'flag', 5, 'Reports to handle', 'Open and under-review reports across locations, reviews and photos.')}
    {queue_card('m4-proposals.html', 'git-pull-request', 4, 'Proposals to review', 'Contributor-proposed changes with a side-by-side diff of old and new values.')}
    </div>

    <section data-od-id="recent-activity" class="mt-6 rounded-2xl border border-line bg-surface p-6 shadow-card sm:p-8">
      <div class="flex flex-wrap items-center justify-between gap-3">
        <h2 class="font-display text-lg font-bold">Recent moderation activity</h2>
        <span class="inline-flex items-center gap-1.5 font-mono text-[11px] uppercase tracking-wide text-muted">{icon('history', 'h-3.5 w-3.5')} Audit trail</span>
      </div>
      <ul class="mt-2 divide-y divide-line">{feed}
      </ul>
      <p class="mt-4 border-t border-line pt-4 text-xs leading-relaxed text-muted">{AUDIT_NOTE}</p>
    </section>

    <details data-od-id="state-previews" class="group mt-8 rounded-2xl border border-dashed border-line bg-bg">
      <summary class="flex cursor-pointer select-none items-center justify-between px-5 py-4 font-mono text-xs uppercase tracking-[0.14em] text-muted">
        Prototype — hidden states
        {icon('chevron-down', 'h-4 w-4 transition-transform group-open:rotate-180')}
      </summary>
      <div class="space-y-6 border-t border-line px-5 py-5">
        <div>
          <p class="mb-2 font-mono text-[11px] uppercase tracking-wide text-muted">Empty queue (all caught up)</p>
          <div class="flex items-center gap-3 rounded-xl border border-line bg-surface p-4">
            {icon('check-circle-2', 'mt-0.5 h-[18px] w-[18px] shrink-0 text-fresh')}
            <p class="text-sm text-fg"><strong class="font-semibold">All queues are clear.</strong> New photos, reports and proposals will appear here as the community submits them.</p>
          </div>
        </div>
        <div>
          <p class="mb-2 font-mono text-[11px] uppercase tracking-wide text-muted">Access denied (non-moderator account)</p>
          <div class="flex items-start gap-3 rounded-xl border border-[oklch(47%_0.17_30_/_0.35)] bg-danger-soft p-4">
            {icon('lock', 'mt-0.5 h-[18px] w-[18px] shrink-0 text-danger')}
            <p class="text-sm leading-relaxed text-fg"><strong class="font-semibold">You don't have access to moderation.</strong> This area is restricted to MODERATOR and ADMIN accounts. If you believe you need access, contact an administrator (§19 — role changes are granted and audited, never self-assigned).</p>
          </div>
        </div>
      </div>
    </details>'''

M1_JS = ''

# ================= M2 — Photo moderation queue =================
PHOTOS = [
    dict(src='images/optimized/street-rack-mint-bike.jpg', w=1200, h=800, spot='Estação Vila Mariana',
         meta='Uploaded 2 days ago · Contributor #1042 · EXIF stripped', note='"New covered rack by the pharmacy."'),
    dict(src='images/optimized/mtb-pair-rack.jpg', w=800, h=1200, spot='Bicicletário Metrô Ana Rosa',
         meta='Uploaded 2 days ago · Contributor #0917 · EXIF stripped', note=''),
    dict(src='images/optimized/hero-bike-parking.jpg', w=2000, h=1125, spot='Estação Vila Mariana',
         meta='Uploaded yesterday · Contributor #1108 · EXIF stripped', note=''),
    dict(src='images/optimized/commuter-portrait.jpg', w=801, h=1200, spot='Parque Ibirapuera — Portão 3',
         meta='Uploaded 3 days ago · Contributor #1215 · EXIF stripped', note=''),
    dict(src='images/optimized/cyclist-crosswalk.jpg', w=1200, h=1200, spot='Rua Domingos de Morais, 1500',
         meta='Uploaded 5 days ago · Contributor #0990 · EXIF stripped', note=''),
]

def photo_card(p, idx):
    return f'''
          <li class="photo-item overflow-hidden rounded-2xl border border-line bg-surface shadow-card" data-photo="{idx}">
            <button type="button" class="photo-view group relative block w-full" data-photo="{idx}" aria-label="View photo full size">
              <img src="{p['src']}" width="{p['w']}" height="{p['h']}" alt="Pending photo for {p['spot']}" class="aspect-4/3 w-full object-cover transition-transform duration-300 group-hover:scale-[1.02]" />
              <span class="absolute left-3 top-3 inline-flex items-center gap-1.5 rounded-full bg-[oklch(18%_0.015_240_/_0.78)] px-2.5 py-1 font-mono text-[11px] font-medium text-white">{icon('eye-off', 'h-3 w-3')} Not public yet</span>
            </button>
            <div class="p-5">
              <p class="text-sm font-semibold">{p['spot']}</p>
              <p class="mt-1 font-mono text-[11px] text-muted">{p['meta']}</p>
              {'<p class="mt-2 text-sm italic text-muted">&ldquo;' + p['note'].strip('"') + '&rdquo;</p>' if p['note'] else ''}
              <div class="mt-4 flex items-center gap-2.5 border-t border-line pt-4">
                <button type="button" class="approve-btn inline-flex h-10 flex-1 items-center justify-center gap-1.5 rounded-lg bg-accent-strong px-3 text-sm font-semibold text-white transition-colors hover:bg-accent-dark">{icon('check', 'h-4 w-4')} Approve</button>
                <button type="button" class="reject-toggle inline-flex h-10 flex-1 items-center justify-center gap-1.5 rounded-lg border border-line px-3 text-sm font-medium text-fg transition-colors hover:border-danger hover:text-danger" aria-expanded="false" aria-controls="reject-panel-{idx}">{icon('x', 'h-4 w-4')} Reject</button>
              </div>
              <div id="reject-panel-{idx}" class="reject-panel hidden border-t border-line pt-4">
                <p class="text-xs font-semibold uppercase tracking-wide text-muted">Reason</p>
                <div class="mt-2 space-y-1.5">
                  {(''.join(f'<label class="flex cursor-pointer items-center gap-2.5 rounded-lg border border-line px-3 py-2 text-sm transition-colors hover:bg-[oklch(20%_0.02_240_/_0.04)]"><input type="radio" name="reject-reason-{idx}" class="accent-[oklch(47%_0.17_30)]" /> {r}</label>' for r in ["Not of this location", "Doesn't show the parking", "Inappropriate content", "Spam or advertising"]))}
                </div>
                <label class="mt-3 block text-xs font-semibold uppercase tracking-wide text-muted" for="reject-note-{idx}">Note to contributor (optional)</label>
                <input id="reject-note-{idx}" type="text" class="mt-1.5 w-full rounded-lg border border-line bg-bg px-3 py-2 text-sm focus:border-accent-strong" placeholder="e.g. Please upload a photo of the rack itself" />
                <button type="button" class="reject-confirm mt-3 inline-flex h-10 w-full items-center justify-center gap-1.5 rounded-lg bg-danger px-3 text-sm font-semibold text-white transition-colors hover:bg-[oklch(40%_0.15_30)]" data-photo="{idx}">{icon('x', 'h-4 w-4')} Reject photo</button>
              </div>
            </div>
          </li>'''

pending_html = ''.join(photo_card(p, i) for i, p in enumerate(PHOTOS))

M2_MAIN = f'''    <div class="mb-8">
      <p class="font-mono text-xs uppercase tracking-[0.14em] text-muted">Moderation · Photos</p>
      <div class="mt-1 flex flex-wrap items-end justify-between gap-3">
        <h1 class="font-display text-3xl font-bold tracking-tight">Photo queue</h1>
        <div class="flex items-center gap-2 font-mono text-[11px] uppercase tracking-wide text-muted">
          <span class="rounded-full border border-line px-2.5 py-1">Pending review</span>{icon('arrow-right', 'h-3 w-3')}
          <span class="rounded-full border border-[oklch(58%_0.13_155_/_0.45)] px-2.5 py-1 text-fresh">Approved</span>
          <span class="rounded-full border border-[oklch(47%_0.17_30_/_0.45)] px-2.5 py-1 text-danger">Rejected</span>
        </div>
      </div>
      <p class="mt-2 max-w-2xl text-sm leading-relaxed text-muted">Newly uploaded photos start as <span class="font-medium text-fg">pending review</span> and are <span class="font-medium text-fg">not publicly visible until approved</span> (§30). Photos already public can still be hidden later.</p>
    </div>

    <section data-od-id="pending-photos" aria-live="polite">
      <h2 class="mb-4 font-display text-lg font-bold">Pending review <span id="pending-count" class="ml-1 align-middle rounded-full bg-[oklch(20%_0.02_240_/_0.06)] px-2.5 py-0.5 font-mono text-xs font-medium text-muted">5</span></h2>
      <ul id="pending-grid" class="grid gap-5 sm:grid-cols-2 lg:grid-cols-3">{pending_html}
      </ul>
      <div id="queue-empty" class="hidden rounded-2xl border border-line bg-surface p-10 text-center shadow-card">
        {icon('check-circle-2', 'mx-auto h-8 w-8 text-fresh')}
        <h3 class="mt-3 font-display text-lg font-bold">Queue is clear</h3>
        <p class="mx-auto mt-1 max-w-sm text-sm text-muted">No photos are waiting for review. New uploads will appear here the moment the community submits them.</p>
      </div>
    </section>

    <section data-od-id="recently-moderated" class="mt-8 rounded-2xl border border-line bg-surface p-6 shadow-card sm:p-8">
      <h2 class="font-display text-lg font-bold">Recently moderated</h2>
      <ul class="mt-2 divide-y divide-line">
        <li class="flex flex-wrap items-center justify-between gap-3 py-3.5">
          <div>
            <p class="text-sm font-medium">Photo approved · Bicicletário Metrô Ana Rosa</p>
            <p class="mt-0.5 text-xs text-muted">By you · yesterday · now public on the location page</p>
          </div>
          <button type="button" class="hide-btn inline-flex h-9 items-center gap-1.5 rounded-lg border border-line px-3 text-xs font-medium text-fg hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon('eye-off', 'h-3.5 w-3.5')} Hide</button>
        </li>
        <li class="flex flex-wrap items-center justify-between gap-3 py-3.5">
          <div>
            <p class="text-sm font-medium">Photo approved · Estação Vila Mariana</p>
            <p class="mt-0.5 text-xs text-muted">By you · 3 days ago</p>
          </div>
          <button type="button" class="hide-btn inline-flex h-9 items-center gap-1.5 rounded-lg border border-line px-3 text-xs font-medium text-fg hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon('eye-off', 'h-3.5 w-3.5')} Hide</button>
        </li>
        <li class="flex flex-wrap items-center justify-between gap-3 py-3.5">
          <div>
            <p class="text-sm font-medium">Photo rejected · Rua Augusta hub</p>
            <p class="mt-0.5 text-xs text-muted">Reason: doesn't show the parking · contributor #0871 notified · 4 days ago</p>
          </div>
          <button type="button" class="restore-btn inline-flex h-9 items-center gap-1.5 rounded-lg border border-line px-3 text-xs font-medium text-fg hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon('undo-2', 'h-3.5 w-3.5')} Restore</button>
        </li>
      </ul>
      <p class="mt-4 border-t border-line pt-4 text-xs leading-relaxed text-muted">{AUDIT_NOTE}</p>
    </section>

    <details data-od-id="state-previews" class="group mt-8 rounded-2xl border border-dashed border-line bg-bg">
      <summary class="flex cursor-pointer select-none items-center justify-between px-5 py-4 font-mono text-xs uppercase tracking-[0.14em] text-muted">
        Prototype — hidden states
        {icon('chevron-down', 'h-4 w-4 transition-transform group-open:rotate-180')}
      </summary>
      <div class="space-y-4 border-t border-line px-5 py-5 text-sm leading-relaxed text-muted">
        <p><strong class="font-semibold text-fg">Approve / reject flows are live:</strong> approving removes the card from the queue and logs it under "Recently moderated"; rejecting expands a reason panel first, and the contributor sees the outcome on their contribution history (C5) with the reason attached.</p>
        <p><strong class="font-semibold text-fg text-danger">Duplicate-check note:</strong> if the same photo file was already approved for another location, the card would show a "Possible duplicate of…" warning before the decision, linking both entries.</p>
      </div>
    </details>

    <div id="lightbox" class="fixed inset-0 z-50 hidden items-center justify-center bg-[oklch(18%_0.015_240_/_0.92)] p-5" role="dialog" aria-modal="true" aria-label="Photo preview">
      <figure class="max-h-full max-w-3xl">
        <img id="lightbox-img" src="" alt="" width="1200" height="800" class="max-h-[80vh] w-auto max-w-full rounded-xl object-contain" />
        <figcaption id="lightbox-cap" class="mt-3 text-center text-sm text-[oklch(98%_0.004_240_/_0.85)]"></figcaption>
      </figure>
      <button id="lightbox-close" type="button" class="absolute right-5 top-5 grid h-10 w-10 place-items-center rounded-full bg-[oklch(100%_0_0_/_0.12)] text-white hover:bg-[oklch(100%_0_0_/_0.22)]" aria-label="Close preview">{icon('x', 'h-5 w-5')}</button>
    </div>'''

M2_JS = '''
    /* ---------- M2: photo queue actions + lightbox ---------- */
    var pendingGrid = document.getElementById('pending-grid');
    var queueEmpty = document.getElementById('queue-empty');
    var pendingCount = document.getElementById('pending-count');

    function refreshCount() {
      var left = pendingGrid.querySelectorAll('.photo-item').length;
      pendingCount.textContent = left;
      if (left === 0) { queueEmpty.classList.remove('hidden'); }
    }

    function moderatedRow(text, detail, actionHtml) {
      var list = document.querySelector('#recently-moderated ul');
      var li = document.createElement('li');
      li.className = 'flex flex-wrap items-center justify-between gap-3 py-3.5';
      li.innerHTML = '<div><p class="text-sm font-medium">' + text + '</p><p class="mt-0.5 text-xs text-muted">' + detail + '</p></div>' + actionHtml;
      list.insertBefore(li, list.firstChild);
    }

    pendingGrid.addEventListener('click', function (e) {
      var approve = e.target.closest('.approve-btn');
      if (approve) {
        var item = approve.closest('.photo-item');
        var spot = item.querySelector('p').textContent;
        item.remove();
        moderatedRow('Photo approved · ' + spot, 'By you · just now · now public on the location page',
          '<button type="button" class="hide-btn inline-flex h-9 items-center gap-1.5 rounded-lg border border-line px-3 text-xs font-medium text-fg hover:bg-[oklch(20%_0.02_240_/_0.04)]">' + document.querySelector('.hide-btn').innerHTML + '</button>');
        flash('Photo approved and published. The action was recorded in the audit trail.');
        refreshCount();
        return;
      }
      var toggle = e.target.closest('.reject-toggle');
      if (toggle) {
        var panel = document.getElementById(toggle.getAttribute('aria-controls'));
        var open = !panel.classList.toggle('hidden');
        toggle.setAttribute('aria-expanded', String(open));
        toggle.textContent = open ? 'Cancel' : 'Reject';
        return;
      }
      var confirm = e.target.closest('.reject-confirm');
      if (confirm) {
        var item = confirm.closest('.photo-item');
        var spot = item.querySelector('p').textContent;
        var radios = item.querySelectorAll('input[type="radio"]');
        var reason = 'No reason given';
        for (var i = 0; i < radios.length; i++) { if (radios[i].checked) { reason = radios[i].nextSibling.textContent.trim(); } }
        var note = item.querySelector('input[type="text"]').value.trim();
        item.remove();
        moderatedRow('Photo rejected · ' + spot, 'Reason: ' + reason + (note ? ' · "' + note + '"' : '') + ' · contributor notified · just now',
          '<button type="button" class="restore-btn inline-flex h-9 items-center gap-1.5 rounded-lg border border-line px-3 text-xs font-medium text-fg hover:bg-[oklch(20%_0.02_240_/_0.04)]">' + document.querySelector('.restore-btn').innerHTML + '</button>');
        flash('Photo rejected. The contributor was notified with the reason. Recorded in the audit trail.');
        refreshCount();
      }
    });

    document.querySelectorAll('.hide-btn, .restore-btn').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var isHide = btn.classList.contains('hide-btn');
        flash(isHide ? 'Photo hidden from the location page. Recorded in the audit trail.'
                     : 'Photo restored to the public gallery. Recorded in the audit trail.');
        btn.disabled = true;
        btn.classList.add('opacity-50');
      });
    });

    var lightbox = document.getElementById('lightbox');
    var lbImg = document.getElementById('lightbox-img');
    var lbCap = document.getElementById('lightbox-cap');
    document.querySelectorAll('.photo-view').forEach(function (btn) {
      btn.addEventListener('click', function () {
        var img = btn.querySelector('img');
        var item = btn.closest('.photo-item');
        lbImg.src = img.src;
        lbImg.alt = img.alt;
        lbCap.textContent = item.querySelector('p').textContent + ' — ' + item.querySelector('p + p').textContent;
        lightbox.classList.remove('hidden');
        lightbox.classList.add('flex');
        document.getElementById('lightbox-close').focus();
      });
    });
    function closeLb() { lightbox.classList.add('hidden'); lightbox.classList.remove('flex'); }
    document.getElementById('lightbox-close').addEventListener('click', closeLb);
    lightbox.addEventListener('click', function (e) { if (e.target === lightbox) closeLb(); });
    document.addEventListener('keydown', function (e) { if (e.key === 'Escape' && !lightbox.classList.contains('hidden')) closeLb(); });'''

# ================= M3 — Reports queue =================
def report_row(state, ic, reason, target, detail, meta, own=False):
    chip = state_chip(state)
    actions = ('<div class="flex items-center gap-2">'
               + f'<button type="button" class="start-review-btn inline-flex h-9 items-center gap-1.5 rounded-lg border border-line px-3 text-xs font-medium text-fg hover:bg-[oklch(20%_0.02_240_/_0.04)]">{icon("search", "h-3.5 w-3.5")} Start review</button>'
               + f'<button type="button" class="resolve-toggle inline-flex h-9 items-center gap-1.5 rounded-lg border border-line px-3 text-xs font-medium text-fg hover:bg-[oklch(20%_0.02_240_/_0.04)]" aria-expanded="false">{icon("check", "h-3.5 w-3.5")} Resolve</button>'
               + f'<button type="button" class="dismiss-btn inline-flex h-9 items-center gap-1.5 rounded-lg border border-line px-3 text-xs font-medium text-muted hover:border-fg hover:text-fg">{icon("x", "h-3.5 w-3.5")} Dismiss</button>'
               + '</div>')
    if own:
        actions = ('<div class="flex items-center gap-2 rounded-lg border border-dashed border-line px-3 py-2 text-xs text-muted">'
                   + icon('circle-help', 'h-3.5 w-3.5 shrink-0')
                   + '<span>You filed this report — another moderator must handle it (§43).</span></div>')
    return f'''
        <li class="report-item rounded-2xl border border-line bg-surface p-5 shadow-card" data-state="{state}"{" data-own=\"true\"" if own else ""}>
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div class="flex items-start gap-3">
              <span class="grid h-9 w-9 shrink-0 place-items-center rounded-lg bg-danger-soft text-danger">{icon(ic, 'h-4 w-4')}</span>
              <div>
                <p class="text-sm font-semibold">{reason}</p>
                <p class="mt-0.5 text-sm text-muted">{target}</p>
              </div>
            </div>
            {chip}
          </div>
          <p class="mt-3 border-l-2 border-line pl-3 text-sm leading-relaxed text-muted">{detail}</p>
          <p class="mt-2 font-mono text-[11px] text-muted">{meta}</p>
          <div class="mt-4 border-t border-line pt-4">{actions}</div>
          <div class="resolve-panel hidden border-t border-line pt-4">
            <p class="text-xs font-semibold uppercase tracking-wide text-muted">Outcome</p>
            <div class="mt-2 space-y-1.5">
              <label class="flex cursor-pointer items-center gap-2.5 rounded-lg border border-line px-3 py-2 text-sm hover:bg-[oklch(20%_0.02_240_/_0.04)]"><input type="radio" name="outcome" class="accent-[oklch(45%_0.11_170)]" checked /> Confirmed — content corrected or removed</label>
              <label class="flex cursor-pointer items-center gap-2.5 rounded-lg border border-line px-3 py-2 text-sm hover:bg-[oklch(20%_0.02_240_/_0.04)]"><input type="radio" name="outcome" class="accent-[oklch(45%_0.11_170)]" /> Confirmed — location invalidated (§44)</label>
              <label class="flex cursor-pointer items-center gap-2.5 rounded-lg border border-line px-3 py-2 text-sm hover:bg-[oklch(20%_0.02_240_/_0.04)]"><input type="radio" name="outcome" class="accent-[oklch(45%_0.11_170)]" /> Not confirmed — content is accurate</label>
            </div>
            <div class="mt-3 flex gap-2">
              <button type="button" class="resolve-confirm inline-flex h-10 flex-1 items-center justify-center gap-1.5 rounded-lg bg-accent-strong px-3 text-sm font-semibold text-white hover:bg-accent-dark">{icon('check-check', 'h-4 w-4')} Resolve report</button>
              <button type="button" class="resolve-cancel inline-flex h-10 items-center rounded-lg border border-line px-3 text-sm font-medium text-muted hover:text-fg">Cancel</button>
            </div>
          </div>
        </li>'''

open_reports = (
    report_row('open', 'coins', 'Incorrect price', 'Location: <a href="p3-parking-details.html" class="font-medium text-accent-strong hover:underline">Estação Vila Mariana</a>',
               'The sign at the entrance says R$ 4 per hour, but the location page shows R$ 5.',
               'Reported 6 h ago · Reporter #1224 · Report #r-231'),
    report_row('open', 'search-x', 'Nonexistent parking', 'Location: <a href="p2-search.html" class="font-medium text-accent-strong hover:underline">Rua Augusta, 892</a>',
               'This rack was removed during construction over a month ago. The spot still appears in search results.',
               'Reported yesterday · Reporter #1204 · Report #r-230'),
    report_row('open', 'clipboard-list', 'Duplicate', 'Location: <a href="p2-search.html" class="font-medium text-accent-strong hover:underline">Bicicletário Paulista</a>',
               'This looks like the same rack already listed as "Estação Paulista" — same block, same photos.',
               'Reported 2 days ago · Reporter #0956 · Report #r-229'),
    report_row('open', 'triangle-alert', 'Inappropriate review', 'Review on <a href="p3-parking-details.html" class="font-medium text-accent-strong hover:underline">Estação Vila Mariana</a>',
               'The review contains abusive language directed at another contributor.',
               'Reported 3 days ago · Filed by you (Ana) · Report #r-228', own=True),
    report_row('review', 'clock', 'Incorrect hours', 'Location: <a href="p3-parking-details.html" class="font-medium text-accent-strong hover:underline">Bicicletário Metrô Ana Rosa</a>',
               'The station opens at 05:30 on weekdays, not 06:00 as listed. Confirmed with the station attendant.',
               'Reported 2 days ago · Reporter #1013 · Under review by moderator #M-0032 since yesterday'),
)
history_rows = '''
        <li class="flex flex-wrap items-center justify-between gap-3 py-3.5">
          <div>
            <p class="text-sm font-medium">Incorrect price · Estação Vila Mariana</p>
            <p class="mt-0.5 text-xs text-muted">Outcome: confirmed — cost corrected to R$ 4/hour · by moderator #M-0032 · yesterday</p>
          </div>
          {resolved_chip}
        </li>
        <li class="flex flex-wrap items-center justify-between gap-3 py-3.5">
          <div>
            <p class="text-sm font-medium">Spam · Review on Estação Paulista</p>
            <p class="mt-0.5 text-xs text-muted">Outcome: confirmed — review hidden, contributor warned · by you · 3 days ago</p>
          </div>
          {resolved_chip}
        </li>
        <li class="flex flex-wrap items-center justify-between gap-3 py-3.5">
          <div>
            <p class="text-sm font-medium">Inappropriate photo · Parque Ibirapuera</p>
            <p class="mt-0.5 text-xs text-muted">Dismissed: photo shows the rack at the edge of the frame — kept public · by moderator #M-0011 · 5 days ago</p>
          </div>
          {dismissed_chip}
        </li>'''
history_rows = history_rows.replace('{resolved_chip}', state_chip('resolved')).replace('{dismissed_chip}', state_chip('dismissed'))

M3_MAIN = f'''    <div class="mb-8">
      <p class="font-mono text-xs uppercase tracking-[0.14em] text-muted">Moderation · Reports</p>
      <h1 class="mt-1 font-display text-3xl font-bold tracking-tight">Reports queue</h1>
      <p class="mt-2 max-w-2xl text-sm leading-relaxed text-muted">Reports move through <span class="font-medium text-fg">Open → Under review → Resolved or Dismissed</span>. Reports can't be resolved or dismissed by the person who filed them (§43).</p>
    </div>

    <div data-od-id="report-tabs" class="mb-5 flex flex-wrap gap-2" role="group" aria-label="Filter by state">
      <button type="button" class="state-tab rounded-full border border-fg bg-fg px-4 py-2 text-sm font-medium text-bg" data-state="all" aria-pressed="true">All <span class="ml-1 font-mono text-xs opacity-70">8</span></button>
      <button type="button" class="state-tab rounded-full border border-line bg-surface px-4 py-2 text-sm font-medium text-fg hover:border-fg" data-state="open" aria-pressed="false">Open <span class="ml-1 font-mono text-xs text-muted">4</span></button>
      <button type="button" class="state-tab rounded-full border border-line bg-surface px-4 py-2 text-sm font-medium text-fg hover:border-fg" data-state="review" aria-pressed="false">Under review <span class="ml-1 font-mono text-xs text-muted">1</span></button>
      <button type="button" class="state-tab rounded-full border border-line bg-surface px-4 py-2 text-sm font-medium text-fg hover:border-fg" data-state="resolved" aria-pressed="false">Resolved <span class="ml-1 font-mono text-xs text-muted">2</span></button>
      <button type="button" class="state-tab rounded-full border border-line bg-surface px-4 py-2 text-sm font-medium text-fg hover:border-fg" data-state="dismissed" aria-pressed="false">Dismissed <span class="ml-1 font-mono text-xs text-muted">1</span></button>
    </div>

    <ul id="report-list" class="space-y-4">{ ''.join(open_reports) }
    </ul>

    <section data-od-id="report-history" class="mt-8 rounded-2xl border border-line bg-surface p-6 shadow-card sm:p-8">
      <h2 class="font-display text-lg font-bold">Resolved &amp; dismissed history</h2>
      <ul class="mt-2 divide-y divide-line">{history_rows}
      </ul>
      <p class="mt-4 border-t border-line pt-4 text-xs leading-relaxed text-muted">{AUDIT_NOTE}</p>
    </section>

    <details data-od-id="state-previews" class="group mt-8 rounded-2xl border border-dashed border-line bg-bg">
      <summary class="flex cursor-pointer select-none items-center justify-between px-5 py-4 font-mono text-xs uppercase tracking-[0.14em] text-muted">
        Prototype — hidden states
        {icon('chevron-down', 'h-4 w-4 transition-transform group-open:rotate-180')}
      </summary>
      <div class="space-y-4 border-t border-line px-5 py-5 text-sm leading-relaxed text-muted">
        <p><strong class="font-semibold text-fg">Flows are live:</strong> "Start review" moves a report to Under review; "Resolve" opens an outcome panel and files the decision; "Dismiss" closes it with the dismissal reason on the record.</p>
        <p><strong class="font-semibold text-fg">Self-report rule:</strong> the "Inappropriate review" report above was filed by the signed-in moderator, so its actions are disabled for them — it must be handled by another moderator (§43). In the production app, the reporter's identity is hidden from the queue; here one report is labelled only to demonstrate the rule.</p>
      </div>
    </details>'''

M3_JS = '''
    /* ---------- M3: report queue actions ---------- */
    var reportList = document.getElementById('report-history');
    var tabsWrap = document.querySelector('[data-od-id="report-tabs"]');
    var reportUl = document.querySelector('ul.space-y-4');

    tabsWrap.addEventListener('click', function (e) {
      var tab = e.target.closest('.state-tab');
      if (!tab) return;
      tabsWrap.querySelectorAll('.state-tab').forEach(function (t) {
        var on = t === tab;
        t.setAttribute('aria-pressed', String(on));
        t.className = 'state-tab rounded-full px-4 py-2 text-sm font-medium border ' + (on ? 'border-fg bg-fg text-bg' : 'border-line bg-surface text-fg hover:border-fg');
      });
      var state = tab.dataset.state;
      document.querySelectorAll('.report-item').forEach(function (li) {
        li.classList.toggle('hidden', state !== 'all' && li.dataset.state !== state);
      });
    });

    var mainList = document.getElementById('content').querySelector('.report-item').parentNode;

    mainList.addEventListener('click', function (e) {
      var start = e.target.closest('.start-review-btn');
      if (start) {
        var li = start.closest('.report-item');
        li.dataset.state = 'review';
        var chip = li.querySelector('.rounded-full.border');
        chip.textContent = 'Under review';
        chip.className = 'inline-flex items-center gap-1.5 rounded-full border border-line px-2.5 py-1 text-xs font-medium bg-[oklch(56%_0.12_170_/_0.12)] text-accent-strong';
        start.remove();
        flash('Report moved to Under review. Recorded in the audit trail.');
        return;
      }
      var toggle = e.target.closest('.resolve-toggle');
      if (toggle) {
        var panel = toggle.closest('.report-item').querySelector('.resolve-panel');
        var open = !panel.classList.toggle('hidden');
        toggle.setAttribute('aria-expanded', String(open));
        return;
      }
      var cancel = e.target.closest('.resolve-cancel');
      if (cancel) { cancel.closest('.resolve-panel').classList.add('hidden'); return; }
      var confirmBtn = e.target.closest('.resolve-confirm');
      if (confirmBtn) {
        var li = confirmBtn.closest('.report-item');
        var title = li.querySelector('p').textContent;
        var outcome = li.querySelector('input[type="radio"]:checked').parentNode.textContent.trim();
        li.remove();
        var list = document.querySelector('[data-od-id="report-history"] ul');
        var li2 = document.createElement('li');
        li2.className = 'flex flex-wrap items-center justify-between gap-3 py-3.5';
        li2.innerHTML = '<div><p class="text-sm font-medium">' + title + '</p><p class="mt-0.5 text-xs text-muted">Outcome: ' + outcome + ' · by you · just now</p></div>'
          + '<span class="inline-flex items-center gap-1.5 rounded-full border border-line px-2.5 py-1 text-xs font-medium bg-[oklch(58%_0.13_155_/_0.12)] text-fresh">Resolved</span>';
        list.insertBefore(li2, list.firstChild);
        flash('Report resolved. The reporter and affected contributors were notified. Recorded in the audit trail.');
        return;
      }
      var dismiss = e.target.closest('.dismiss-btn');
      if (dismiss) {
        var li = dismiss.closest('.report-item');
        var title = li.querySelector('p').textContent;
        li.remove();
        var list = document.querySelector('[data-od-id="report-history"] ul');
        var li2 = document.createElement('li');
        li2.className = 'flex flex-wrap items-center justify-between gap-3 py-3.5';
        li2.innerHTML = '<div><p class="text-sm font-medium">' + title + '</p><p class="mt-0.5 text-xs text-muted">Dismissed by you · just now</p></div>'
          + '<span class="inline-flex items-center gap-1.5 rounded-full border border-line px-2.5 py-1 text-xs font-medium bg-[oklch(20%_0.02_240_/_0.06)] text-muted">Dismissed</span>';
        list.insertBefore(li2, list.firstChild);
        flash('Report dismissed. The decision and reason are on the audit record.');
      }
    });'''

# ================= M4 — Proposal review =================
def proposal_card(target, href, field, old, new, submitter, when, note='', conflict=''):
    return f'''
        <li class="proposal-item rounded-2xl border border-line bg-surface p-5 shadow-card sm:p-6">
          <div class="flex flex-wrap items-start justify-between gap-3">
            <div>
              <p class="font-mono text-[11px] uppercase tracking-wide text-muted">{field}</p>
              <p class="mt-1 text-sm font-semibold">Location: <a href="p3-parking-details.html" class="text-accent-strong hover:underline">{target}</a></p>
            </div>
            <span class="inline-flex items-center gap-1.5 rounded-full border border-[oklch(68%_0.12_75_/_0.45)] bg-[oklch(68%_0.12_75_/_0.12)] px-2.5 py-1 text-xs font-medium text-aging">Pending</span>
          </div>
          <div class="mt-4 rounded-xl border border-line bg-bg p-4 font-mono text-sm">
            <p class="flex flex-wrap items-center gap-2"><span class="text-muted line-through">{old}</span>{icon('arrow-right', 'h-3.5 w-3.5 shrink-0 text-muted')}<span class="font-semibold text-fg">{new}</span></p>
          </div>
          {'<p class="mt-3 flex items-start gap-2 rounded-xl border border-[oklch(68%_0.12_75_/_0.40)] bg-[oklch(68%_0.12_75_/_0.10)] p-3 text-sm leading-relaxed text-fg">' + icon('circle-alert', 'mt-0.5 h-4 w-4 shrink-0 text-aging') + '<span>' + conflict + '</span></p>' if conflict else ''}
          {'<p class="mt-3 text-sm italic text-muted">"' + note + '"</p>' if note else ''}
          <p class="mt-3 font-mono text-[11px] text-muted">Proposed by contributor {submitter} · {when}</p>
          <div class="mt-4 flex flex-wrap items-center gap-2 border-t border-line pt-4">
            <button type="button" class="approve-prop-btn inline-flex h-10 items-center gap-1.5 rounded-lg bg-accent-strong px-4 text-sm font-semibold text-white transition-colors hover:bg-accent-dark">{icon('check', 'h-4 w-4')} Approve &amp; apply</button>
            <button type="button" class="reject-prop-toggle inline-flex h-10 items-center gap-1.5 rounded-lg border border-line px-4 text-sm font-medium text-fg transition-colors hover:border-danger hover:text-danger" aria-expanded="false">{icon('x', 'h-4 w-4')} Reject</button>
            <span class="ml-auto text-xs text-muted">Previous value stays in the location's history (§37)</span>
          </div>
          <div class="reject-prop-panel hidden border-t border-line pt-4">
            <label class="text-xs font-semibold uppercase tracking-wide text-muted" for="preject-note">Reason to contributor</label>
            <input id="preject-note" type="text" class="mt-1.5 w-full rounded-lg border border-line bg-bg px-3 py-2 text-sm focus:border-accent-strong" placeholder="e.g. Please attach a photo of the new price sign" />
            <div class="mt-3 flex gap-2">
              <button type="button" class="reject-prop-confirm inline-flex h-10 flex-1 items-center justify-center gap-1.5 rounded-lg bg-danger px-3 text-sm font-semibold text-white hover:bg-[oklch(40%_0.15_30)]">{icon('x', 'h-4 w-4')} Reject proposal</button>
              <button type="button" class="reject-prop-cancel inline-flex h-10 items-center rounded-lg border border-line px-3 text-sm font-medium text-muted hover:text-fg">Cancel</button>
            </div>
          </div>
        </li>'''

proposals_html = (
    proposal_card('Estação Vila Mariana', '#', 'Cost', 'R$ 5 / hour', 'R$ 6 / hour', '#1042', 'yesterday',
                  note='The hourly rate went up this month — new sign at the entrance.'),
    proposal_card('Bicicletário Metrô Ana Rosa', '#', 'Opening hours', 'Mon–Fri 06:00–22:00', 'Mon–Fri 05:30–22:00', '#0917', '2 days ago',
                  note='The station gate opens at 05:30 — confirmed with the attendant.'),
    proposal_card('Estação Paulista', '#', 'Security', 'Covered, CCTV', 'Covered, CCTV, Staffed station', '#1108', '3 days ago'),
    proposal_card('Rua Domingos de Morais, 1500', '#', 'Address', 'Rua Domingos de Morais, 1500', 'Rua Domingos de Morais, 1450', '#0990', '4 days ago',
                  conflict='Two recent "I parked here" confirmations (last week) still place this rack at number 1500. The signals are kept visible rather than averaged away (§106).'),
)

reviewed_rows = '''
        <li class="flex flex-wrap items-center justify-between gap-3 py-3.5">
          <div>
            <p class="text-sm font-medium">Cost change · Estação Vila Mariana</p>
            <p class="mt-0.5 text-xs text-muted">R$ 5 → R$ 4 / hour · approved by you · yesterday · old value kept in history</p>
          </div>
          {ok}
        </li>
        <li class="flex flex-wrap items-center justify-between gap-3 py-3.5">
          <div>
            <p class="text-sm font-medium">Security chips · Bicicletário Paulista</p>
            <p class="mt-0.5 text-xs text-muted">Rejected: no evidence of an attendant on site · reason sent to contributor · 2 days ago</p>
          </div>
          {no}
        </li>
        <li class="flex flex-wrap items-center justify-between gap-3 py-3.5">
          <div>
            <p class="text-sm font-medium">Name change · "Bike Rack Rua Augusta"</p>
            <p class="mt-0.5 text-xs text-muted">Approved with modification — name normalized to "Rua Augusta bike rack" · 5 days ago</p>
          </div>
          {ok}
        </li>'''
REJECTED_CHIP = '<span class="inline-flex items-center gap-1.5 rounded-full border border-line bg-danger-soft px-2.5 py-1 text-xs font-medium text-danger">Rejected</span>'
reviewed_rows = reviewed_rows.replace('{ok}', state_chip('resolved')).replace('{no}', REJECTED_CHIP)

M4_MAIN = f'''    <div class="mb-8">
      <p class="font-mono text-xs uppercase tracking-[0.14em] text-muted">Moderation · Proposals</p>
      <h1 class="mt-1 font-display text-3xl font-bold tracking-tight">Proposal review</h1>
      <p class="mt-2 max-w-2xl text-sm leading-relaxed text-muted">Changes proposed by contributors (D2). Approving applies the new value; the previous value is retained in the location's history instead of being silently overwritten (§37, §107).</p>
    </div>

    <ul class="space-y-4">{ ''.join(proposals_html) }
    </ul>

    <section data-od-id="proposal-history" class="mt-8 rounded-2xl border border-line bg-surface p-6 shadow-card sm:p-8">
      <h2 class="font-display text-lg font-bold">Recently reviewed</h2>
      <ul class="mt-2 divide-y divide-line">{reviewed_rows}
      </ul>
      <p class="mt-4 border-t border-line pt-4 text-xs leading-relaxed text-muted">{AUDIT_NOTE}</p>
    </section>

    <details data-od-id="state-previews" class="group mt-8 rounded-2xl border border-dashed border-line bg-bg">
      <summary class="flex cursor-pointer select-none items-center justify-between px-5 py-4 font-mono text-xs uppercase tracking-[0.14em] text-muted">
        Prototype — hidden states
        {icon('chevron-down', 'h-4 w-4 transition-transform group-open:rotate-180')}
      </summary>
      <div class="space-y-4 border-t border-line px-5 py-5 text-sm leading-relaxed text-muted">
        <p><strong class="font-semibold text-fg">Flows are live:</strong> "Approve &amp; apply" moves the proposal to the reviewed list and applies the value; "Reject" asks for a reason that goes back to the contributor (visible on their C5 history).</p>
        <p><strong class="font-semibold text-fg">Modify path:</strong> moderators can also approve with modifications (see "name change" in the history) — the applied value and the original proposal are both kept on the audit record.</p>
      </div>
    </details>'''

M4_JS = '''
    /* ---------- M4: proposal review actions ---------- */
    var list = document.getElementById('content').querySelector('ul.space-y-4');
    var histList = document.querySelector('[data-od-id="proposal-history"] ul');

    function addHistory(title, detail, chipLabel) {
      var li = document.createElement('li');
      li.className = 'flex flex-wrap items-center justify-between gap-3 py-3.5';
      li.innerHTML = '<div><p class="text-sm font-medium">' + title + '</p><p class="mt-0.5 text-xs text-muted">' + detail + '</p></div>'
        + '<span class="inline-flex items-center gap-1.5 rounded-full border border-line px-2.5 py-1 text-xs font-medium ' + (chipLabel === 'Approved' ? 'bg-[oklch(58%_0.13_155_/_0.12)] text-fresh' : 'bg-[oklch(20%_0.02_240_/_0.06)] text-muted') + '">' + chipLabel + '</span>';
      histList.insertBefore(li, histList.firstChild);
    }

    list.addEventListener('click', function (e) {
      var approve = e.target.closest('.approve-prop-btn');
      if (approve) {
        var li = approve.closest('.proposal-item');
        var field = li.querySelector('p').textContent;
        var target = li.querySelectorAll('p')[1].textContent.replace('Location: ', '');
        var diff = li.querySelector('.font-mono span.font-semibold').textContent;
        li.remove();
        addHistory(field + ' · ' + target, diff + ' · approved by you · just now · old value kept in history', 'Approved');
        flash('Proposal applied. The change is live and recorded in the audit trail.');
        return;
      }
      var toggle = e.target.closest('.reject-prop-toggle');
      if (toggle) {
        var panel = toggle.closest('.proposal-item').querySelector('.reject-prop-panel');
        var open = !panel.classList.toggle('hidden');
        toggle.setAttribute('aria-expanded', String(open));
        return;
      }
      var cancel = e.target.closest('.reject-prop-cancel');
      if (cancel) { cancel.closest('.reject-prop-panel').classList.add('hidden'); return; }
      var confirm = e.target.closest('.reject-prop-confirm');
      if (confirm) {
        var li = confirm.closest('.proposal-item');
        var field = li.querySelector('p').textContent;
        var target = li.querySelectorAll('p')[1].textContent.replace('Location: ', '');
        var note = li.querySelector('input[type="text"]').value.trim();
        li.remove();
        addHistory(field + ' · ' + target, 'Rejected' + (note ? ' — "' + note + '"' : '') + ' · by you · just now', 'Rejected');
        flash('Proposal rejected. The contributor was notified with your reason. Recorded in the audit trail.');
      }
    });'''

page('m1-moderation.html', 'Moderation overview', 'Moderation dashboard: queue counts and recent activity.', 'M1 Moderation overview', 'm1-moderation.html', M1_MAIN, M1_JS)
page('m2-photos.html', 'Photo moderation queue', 'Review pending photos before they become publicly visible.', 'M2 Photo queue', 'm2-photos.html', M2_MAIN, M2_JS)
page('m3-reports.html', 'Reports queue', 'Review, resolve and dismiss community reports.', 'M3 Reports queue', 'm3-reports.html', M3_MAIN, M3_JS)
page('m4-proposals.html', 'Proposal review', 'Review proposed changes with a diff of old and new values.', 'M4 Proposal review', 'm4-proposals.html', M4_MAIN, M4_JS)
print('done')
