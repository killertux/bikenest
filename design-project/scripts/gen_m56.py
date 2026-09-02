#!/usr/bin/env python3
# Generates M5 (admin users) + M6 (audit log) with an admin shell, and E1/E2 error pages.
import json, re, os

ICON_DIR = 'assets/vendor/icons'
_cache = {}
def icon(name, cls='h-4 w-4'):
    if name not in _cache:
        s = open(os.path.join(ICON_DIR, name + '.svg')).read()
        s = ' '.join(s.split())
        s = re.sub(r'class="lucide[^"]*"', '', s, count=1)
        s = s.replace('<svg ', '<svg aria-hidden="true" ', 1)
        _cache[name] = s
    return _cache[name].replace('<svg ', f'<svg class="{cls}" ', 1)

TOKENS = open('c1-account.html').read()
mm = re.search(r'<script>\s*(/\* Design tokens.*?)</script>', TOKENS, re.S)
TOKENS_JS = mm.group(1)
SHELL_JS = TOKENS[TOKENS.index('/* ---------- Shell'):TOKENS.index('  </script>', TOKENS.index('/* ---------- Shell'))]

MOD_LINKS = [
    ('m1-moderation.html', 'shield', 'Moderation overview'),
    ('m2-photos.html', 'image', 'Photo queue'),
    ('m3-reports.html', 'flag', 'Reports queue'),
    ('m4-proposals.html', 'git-pull-request', 'Proposal review')]
ADMIN_LINKS = [
    ('m5-users.html', 'users', 'User management'),
    ('m6-audit.html', 'history', 'Audit log')]

def header(active):
    def mi(href, ic, label, cur):
        cls = 'font-medium text-fg' if cur else 'text-fg'
        return (f'<a role="menuitem" href="{href}" class="flex items-center gap-2.5 rounded-lg px-3 py-2 text-sm {cls} '
                'hover:bg-[oklch(20%_0.02_240_/_0.05)]">' + icon(ic, 'h-4 w-4 text-muted') + f' {label}</a>')
    def mob(href, label, cur):
        cls = 'font-medium text-fg' if cur else 'text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg'
        return f'<a href="{href}" class="rounded-lg px-3 py-2.5 text-[15px] {cls}">{label}</a>'
    mod_links = ''.join(mi(h, i, l, h == active) for h, i, l in MOD_LINKS)
    admin_links = ''.join(mi(h, i, l, h == active) for h, i, l in ADMIN_LINKS)
    mod_mob = ''.join(mob(h, l, h == active) for h, i, l in MOD_LINKS)
    admin_mob = ''.join(mob(h, l, h == active) for h, i, l in ADMIN_LINKS)
    return f'''
  <header data-od-id="topnav-admin" class="sticky top-0 z-40 border-b border-line bg-[oklch(98%_0.004_240_/_0.9)] backdrop-blur">
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
                <span class="inline-flex items-center gap-1 rounded-full bg-[oklch(45%_0.15_265_/_0.12)] px-2 py-0.5 text-[11px] font-semibold text-[oklch(42%_0.13_265)]">{icon('shield', 'h-3 w-3')} Admin</span>
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
            <p class="px-3 pb-1 pt-2 font-mono text-[11px] uppercase tracking-wide text-muted">Administration</p>
            {admin_links}
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
        <p class="px-3 pb-1 font-mono text-[11px] uppercase tracking-wide text-muted">Administration</p>
        {admin_mob}
      </div>
      <div class="mt-3 flex flex-col gap-1 border-t border-line pt-3">
        <p class="px-3 pb-1 font-mono text-[11px] uppercase tracking-wide text-muted">Your account</p>
        <a href="c1-account.html" class="rounded-lg px-3 py-2.5 text-[15px] text-muted hover:bg-[oklch(20%_0.02_240_/_0.05)] hover:text-fg">Account overview</a>
        <a href="a2-login.html" class="mt-1 rounded-lg px-3 py-2.5 text-[15px] font-medium text-danger hover:bg-danger-soft">Log out</a>
      </div>
    </div>
  </header>'''

def tabs(active):
    def t(href, label):
        cur = ' border-accent-strong font-medium text-fg' if active == href else ' border-transparent text-muted hover:border-line hover:text-fg'
        return f'<a href="{href}"{" aria-current=\"page\"" if active == href else ""} class="whitespace-nowrap border-b-2{cur} py-3.5 text-sm">{label}</a>'
    return ('<nav data-od-id="moderation-tabs" aria-label="Moderation and administration sections" class="border-b border-line bg-bg">'
            '<div class="mx-auto flex max-w-shell gap-6 overflow-x-auto px-5 lg:px-8">'
            + ''.join(t(h, l) for h, i, l in MOD_LINKS + ADMIN_LINKS)
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

def page(fname, title, desc, tag, active, main, extra_js):
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
{SHELL_JS}
{extra_js}
  </script>

</body>
</html>
'''
    open(fname, 'w').write(html)
    print('wrote', fname, len(html.splitlines()), 'lines')

# ================= M5 — User management (admin) =================
USERS = [
    ('Ana Ribeiro', 'ana.ribeiro@example.com', 'ADMIN', 'active', 31, 'Jan 2026', 'self'),
    ('Bruno Costa', 'bruno.costa@example.com', 'MODERATOR', 'active', 42, 'Feb 2026', ''),
    ('Carla Mendes', 'carla.mendes@example.com', 'USER', 'active', 17, 'Mar 2026', ''),
    ('Diego Ferreira', 'diego.ferreira@example.com', 'USER', 'suspended', 3, 'Apr 2026', 'Suspended for review spam — audit #2231'),
    ('Elena Souza', 'elena.souza@example.com', 'USER', 'pending', 0, 'Jun 2026', ''),
    ('Felipe Lima', 'felipe.lima@example.com', 'MODERATOR', 'active', 68, 'Jan 2026', ''),
    ('Gustavo Rocha', 'gustavo.rocha@example.com', 'USER', 'active', 9, 'May 2026', ''),
    ('Helena Prado', 'helena.prado@example.com', 'USER', 'active', 5, 'Jun 2026', ''),
]

def state_chip(state):
    if state == 'suspended':
        return ('<span class="inline-flex items-center gap-1.5 rounded-full border border-[oklch(55%_0.19_25_/_0.35)] bg-danger-soft px-2.5 py-1 text-xs font-medium text-danger">'
                + icon('ban', 'h-3.5 w-3.5') + 'Suspended</span>')
    if state == 'pending':
        return ('<span class="inline-flex items-center gap-1.5 rounded-full border border-[oklch(68%_0.12_75_/_0.45)] bg-[oklch(68%_0.12_75_/_0.12)] px-2.5 py-1 text-xs font-medium text-aging">'
                + icon('mail', 'h-3.5 w-3.5') + 'Pending verification</span>')
    return ('<span class="inline-flex items-center gap-1.5 rounded-full border border-[oklch(58%_0.13_155_/_0.35)] bg-[oklch(58%_0.13_155_/_0.10)] px-2.5 py-1 text-xs font-medium text-fresh">'
            + icon('check-circle-2', 'h-3.5 w-3.5') + 'Active</span>')

def role_chip(role):
    if role == 'ADMIN':
        return ('<span class="inline-flex items-center gap-1 rounded-full bg-[oklch(45%_0.16_265_/_0.12)] px-2 py-0.5 text-[11px] font-semibold text-[oklch(42%_0.14_265)]">'
                + icon('shield', 'h-3 w-3') + 'Admin</span>')
    if role == 'MODERATOR':
        return ('<span class="inline-flex items-center gap-1 rounded-full bg-[oklch(56%_0.12_170_/_0.12)] px-2 py-0.5 text-[11px] font-semibold text-accent-strong">'
                + icon('shield-check', 'h-3 w-3') + 'Moderator</span>')
    return '<span class="inline-flex items-center gap-1 rounded-full bg-[oklch(20%_0.02_240_/_0.06)] px-2 py-0.5 text-[11px] font-semibold text-muted">User</span>'

user_rows = ''
for name, email, role, state, contrib, joined, note in USERS:
    self_row = note == 'self'
    you = ' <span class="ml-1 rounded bg-[oklch(20%_0.02_240_/_0.06)] px-1.5 py-0.5 font-mono text-[10px] uppercase tracking-wide text-muted">You</span>' if self_row else ''
    note_html = ''
    if note and not self_row:
        note_html = f'<p class="mt-1 text-xs leading-relaxed text-danger">{icon("triangle-alert", "mr-1 inline h-3 w-3 -translate-y-px")}{note}</p>'
    if self_row:
        role_cell = (f'<div class="flex items-center gap-1.5">{role_chip(role)}'
                     '<span class="font-mono text-[10px] uppercase tracking-wide text-muted">Own role — locked</span></div>')
        actions = '<span class="font-mono text-[10px] uppercase tracking-wide text-muted">Own account — role locked</span>'
    else:
        role_cell = (f'<div class="flex items-center gap-1.5" data-role-cell>{role_chip(role)}'
                     f'<button type="button" data-action="role" aria-label="Change role for {name}" title="Change role" '
                     'class="grid h-6 w-6 place-items-center rounded-md text-muted transition-colors hover:bg-[oklch(20%_0.02_240_/_0.06)] hover:text-fg">'
                     + icon('settings-2', 'h-3.5 w-3.5') + '</button></div>')
        if state == 'suspended':
            actions = f'<button type="button" data-action="restore" class="rounded-lg border border-line px-2.5 py-1.5 text-xs font-medium text-fg transition-colors hover:bg-[oklch(20%_0.02_240_/_0.05)]">Restore</button>'
        else:
            actions = f'<button type="button" data-action="suspend" class="rounded-lg border border-[oklch(55%_0.19_25_/_0.35)] px-2.5 py-1.5 text-xs font-medium text-danger transition-colors hover:bg-danger-soft">Suspend</button>'
    user_rows += f'''
      <tr data-od-id="user-row-{name.split()[0].lower()}" data-name="{(name + ' ' + email).lower()}" data-state="{state}" class="border-t border-line align-top">
        <td class="px-5 py-4 pr-4">
          <p class="text-sm font-semibold">{name}{you}</p>
          <p class="mt-0.5 text-xs text-muted">{email}</p>
          {note_html}
        </td>
        <td class="state-cell py-4 pr-4">
          <div class="flex flex-col items-start gap-1.5">{role_cell}{state_chip(state)}</div>
        </td>
        <td class="py-4 pr-4 font-mono text-sm text-muted">{contrib}</td>
        <td class="py-4 pr-4 font-mono text-xs text-muted">{joined}</td>
        <td class="py-4 pr-5">
          <div class="flex flex-wrap items-center justify-end gap-2">{actions}</div>
        </td>
      </tr>'''

M5_MAIN = f'''    <div class="mb-8 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
      <div>
        <p class="font-mono text-xs uppercase tracking-[0.14em] text-muted">Administration</p>
        <h1 class="mt-1 font-display text-3xl font-bold tracking-tight">User management</h1>
        <p class="mt-2 max-w-2xl text-sm leading-relaxed text-muted">Account states and role assignment. Role changes require an ADMIN principal and always create an audit event — they are denied by default everywhere else (§19).</p>
      </div>
      <span class="inline-flex w-fit items-center gap-1.5 rounded-full bg-[oklch(45%_0.15_265_/_0.12)] px-3 py-1.5 text-xs font-semibold text-[oklch(42%_0.13_265)]">{icon('shield', 'h-3.5 w-3.5')} Signed in as Admin</span>
    </div>

    <div class="mb-4 flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
      <div class="relative sm:w-80">
        {icon('search', 'pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted')}
        <input id="user-search" type="search" placeholder="Search by name or email…" aria-label="Search users"
               class="w-full rounded-xl border border-line bg-surface py-2.5 pl-10 pr-4 text-sm placeholder:text-muted focus:border-accent-strong" />
      </div>
      <div id="user-filters" class="flex items-center gap-2" role="group" aria-label="Filter by account state">
        <button type="button" data-state="all" class="state-chip rounded-full border border-fg bg-fg px-3 py-1.5 text-xs font-medium text-bg">All · {len(USERS)}</button>
        <button type="button" data-state="active" class="state-chip rounded-full border border-line px-3 py-1.5 text-xs text-muted transition-colors hover:border-fg hover:text-fg">Active</button>
        <button type="button" data-state="suspended" class="state-chip rounded-full border border-line px-3 py-1.5 text-xs text-muted transition-colors hover:border-fg hover:text-fg">Suspended</button>
        <button type="button" data-state="pending" class="state-chip rounded-full border border-line px-3 py-1.5 text-xs text-muted transition-colors hover:border-fg hover:text-fg">Pending</button>
      </div>
    </div>

    <div class="overflow-x-auto rounded-2xl border border-line bg-surface shadow-card">
      <table class="w-full min-w-[780px] text-left">
        <thead>
          <tr class="border-b border-line font-mono text-[11px] uppercase tracking-wide text-muted">
            <th scope="col" class="px-5 py-3 font-medium">User</th>
            <th scope="col" class="py-3 pr-4 font-medium">Role &amp; state</th>
            <th scope="col" class="py-3 pr-4 font-medium">Contributions</th>
            <th scope="col" class="py-3 pr-4 font-medium">Joined</th>
            <th scope="col" class="py-3 pr-5 text-right font-medium">Actions</th>
          </tr>
        </thead>
        <tbody id="user-rows">{user_rows}
        </tbody>
      </table>
      <p id="user-empty" class="hidden px-5 py-10 text-center text-sm text-muted">No users match this search or filter.</p>
    </div>

    <section data-od-id="role-notes" class="mt-6 grid gap-4 sm:grid-cols-2">
      <div class="rounded-2xl border border-line bg-surface p-6 shadow-card">
        <h2 class="flex items-center gap-2 font-display text-base font-bold">{icon('user-plus', 'h-[18px] w-[18px] text-accent-strong')} Granting a role</h2>
        <p class="mt-2 text-sm leading-relaxed text-muted">Use the gear icon on a row to grant or revoke MODERATOR. Every change asks for confirmation, is tied to your ADMIN principal, and writes an audit event. Roles are never granted through self-service settings (§19).</p>
      </div>
      <div class="rounded-2xl border border-line bg-surface p-6 shadow-card">
        <h2 class="flex items-center gap-2 font-display text-base font-bold">{icon('user-x', 'h-[18px] w-[18px] text-danger')} Suspending an account</h2>
        <p class="mt-2 text-sm leading-relaxed text-muted">Suspension is for abusive behaviour (§44): the user is signed out and can no longer sign in or contribute. Their public contributions follow the anonymization rules. Suspension is reversible and audited.</p>
      </div>
    </section>

    <details data-od-id="state-previews" class="group mt-8 rounded-2xl border border-dashed border-line bg-bg">
      <summary class="flex cursor-pointer select-none items-center justify-between px-5 py-4 font-mono text-xs uppercase tracking-[0.14em] text-muted">
        Prototype — hidden states
        {icon('chevron-down', 'h-4 w-4 transition-transform group-open:rotate-180')}
      </summary>
      <div class="space-y-6 border-t border-line px-5 py-5">
        <div>
          <p class="mb-2 font-mono text-[11px] uppercase tracking-wide text-muted">Access denied (MODERATOR opening /admin/users)</p>
          <div class="flex items-start gap-3 rounded-xl border border-line bg-surface p-4">
            {icon('lock', 'mt-0.5 h-[18px] w-[18px] shrink-0 text-danger')}
            <p class="text-sm leading-relaxed text-fg"><strong class="font-semibold">You don't have access to user management.</strong> This area is restricted to ADMIN accounts. Role changes are explicit, audited operations — never self-assigned (§19).</p>
          </div>
        </div>
      </div>
    </details>'''

ICONS = {
    'activeChip': state_chip('active'),
    'suspendedChip': state_chip('suspended'),
    'modChip': role_chip('MODERATOR'),
    'userChip': role_chip('USER'),
    'adminChip': role_chip('ADMIN'),
    'gear': icon('settings-2', 'h-3.5 w-3.5'),
}

M5_JS = '''
    /* ---------- M5: user management ---------- */
    var M5 = __ICONS__;
    (function () {
      var search = document.getElementById('user-search');
      var rows = Array.prototype.slice.call(document.querySelectorAll('#user-rows tr'));
      var stateFilter = 'all';

      function apply() {
        var q = (search.value || '').trim().toLowerCase();
        var visible = 0;
        rows.forEach(function (tr) {
          var okState = stateFilter === 'all' || tr.getAttribute('data-state') === stateFilter;
          var hay = tr.getAttribute('data-name');
          var show = okState && (!q || hay.indexOf(q) !== -1);
          tr.classList.toggle('hidden', !show);
          if (show) visible++;
        });
        document.getElementById('user-empty').classList.toggle('hidden', visible !== 0);
      }
      search.addEventListener('input', apply);

      document.getElementById('user-filters').addEventListener('click', function (e) {
        var btn = e.target.closest('.state-chip');
        if (!btn) return;
        stateFilter = btn.getAttribute('data-state');
        document.querySelectorAll('.state-chip').forEach(function (b) {
          var on = b === btn;
          b.className = 'state-chip rounded-full border px-3 py-1.5 text-xs ' +
            (on ? 'border-fg bg-fg font-medium text-bg' : 'border-line text-muted transition-colors hover:border-fg hover:text-fg');
        });
        apply();
      });

      function setState(tr, chip) {
        var roleCell = tr.querySelector('[data-role-cell]');
        tr.querySelector('.state-cell').innerHTML =
          '<div class="flex flex-col items-start gap-1.5">' + (roleCell ? roleCell.outerHTML : '') + chip + '</div>';
      }

      document.getElementById('user-rows').addEventListener('click', function (e) {
        var susp = e.target.closest('[data-action="suspend"]');
        if (susp && !susp.disabled) {
          var tr = susp.closest('tr');
          var name = tr.querySelector('td p').childNodes[0].textContent.trim();
          if (confirm('Suspend ' + name + '?\\n\\nThe account is signed out immediately and can no longer sign in or contribute. This action is recorded in the audit trail (§44).')) {
            setState(tr, M5.suspendedChip);
            tr.setAttribute('data-state', 'suspended');
            susp.textContent = 'Restore';
            susp.setAttribute('data-action', 'restore');
            susp.className = 'rounded-lg border border-line px-2.5 py-1.5 text-xs font-medium text-fg transition-colors hover:bg-[oklch(20%_0.02_240_/_0.05)]';
            flash(name + ' was suspended. Recorded in the audit trail.');
          }
          return;
        }
        var restore = e.target.closest('[data-action="restore"]');
        if (restore) {
          var tr2 = restore.closest('tr');
          var name2 = tr2.querySelector('td p').childNodes[0].textContent.trim();
          setState(tr2, M5.activeChip);
          tr2.setAttribute('data-state', 'active');
          restore.textContent = 'Suspend';
          restore.setAttribute('data-action', 'suspend');
          restore.className = 'rounded-lg border border-[oklch(55%_0.19_25_/_0.35)] px-2.5 py-1.5 text-xs font-medium text-danger transition-colors hover:bg-danger-soft';
          flash(name2 + ' was restored to active. Recorded in the audit trail.');
        }
      });
    })();'''
M5_JS = M5_JS.replace('__ICONS__', json.dumps(ICONS))

# ================= M6 — Audit log (admin) =================
AUDIT = [
    ('2026-06-12 14:32', 'ana.ribeiro (ADMIN)', 'role.granted', 'carla.mendes — MODERATOR', 'success', 'principal=ana.ribeiro · reason="community moderator rotation"'),
    ('2026-06-12 14:31', 'ana.ribeiro (ADMIN)', 'role.revoked', 'joao.silva — MODERATOR', 'success', 'reason="stepped down at own request"'),
    ('2026-06-12 13:08', 'bruno.costa (MODERATOR)', 'photo.approved', 'Bicicletário Metrô Ana Rosa', 'success', 'photo #4218'),
    ('2026-06-12 12:47', 'system', 'export.download.denied', 'user #0871 → own export link', 'denied', 'link expired 6 h earlier (24 h expiry, §73)'),
    ('2026-06-12 11:47', 'ana.ribeiro (ADMIN)', 'user.suspended', 'Diego Ferreira (#0932)', 'success', 'reason="review spam" · audit #2231'),
    ('2026-06-12 11:47', 'marina.c (USER)', 'review.created', 'Estação Vila Mariana', 'success', 'review #3391 · pending moderation'),
    ('2026-06-12 10:02', 'system', 'token.reset.expired', 'password reset — user #0455', 'denied', 'token age 1 h 12 m (lifetime 1 h, §75)'),
    ('2026-06-11 18:40', 'felipe.lima (MODERATOR)', 'report.resolved', 'Incorrect price — Estação Paulista', 'success', 'cost corrected to R$ 4/hour'),
    ('2026-06-11 16:20', 'system', 'login.failed', 'user "m.oliveira"', 'denied', 'wrong password · attempt 1/5'),
    ('2026-06-11 16:05', 'ana.ribeiro (ADMIN)', 'location.invalidated', 'Bicicletário Rua Augusta (closed)', 'success', 'marked removed · history retained (§37)'),
    ('2026-06-11 11:30', 'carla.mendes (MODERATOR)', 'photo.rejected', "Rua Augusta hub", 'success', "reason: doesn't show the parking"),
    ('2026-06-10 09:14', 'helena.prado (USER)', 'account.deleted', 'self', 'success', 'contributions anonymized (§74) · email removed'),
]

def result_chip(result):
    if result == 'success':
        return ('<span class="inline-flex items-center gap-1.5 rounded-full border border-[oklch(58%_0.13_155_/_0.35)] bg-[oklch(58%_0.13_155_/_0.10)] px-2.5 py-0.5 text-xs font-medium text-fresh">'
                + icon('check', 'h-3 w-3') + 'Success</span>')
    if result == 'denied':
        return ('<span class="inline-flex items-center gap-1.5 rounded-full border border-[oklch(55%_0.19_25_/_0.35)] bg-danger-soft px-2.5 py-0.5 text-xs font-medium text-danger">'
                + icon('x', 'h-3 w-3') + 'Denied</span>')
    return '<span class="inline-flex items-center gap-1.5 rounded-full bg-[oklch(20%_0.02_240_/_0.06)] px-2.5 py-0.5 text-xs font-medium text-muted">Info</span>'

def category(action):
    if action.startswith('role.') or action.startswith('user.') or action.startswith('account.'):
        return 'account'
    if action.startswith('report.') or action.startswith('photo.'):
        return 'moderation'
    if action.startswith('export.') or action.startswith('token.'):
        return 'privacy'
    return 'auth'

audit_rows = ''
for when, actor, action, target, result, meta in AUDIT:
    cat_key = ('account' if action.startswith(('role.', 'user.', 'account.'))
               else 'moderation' if action.startswith(('report.', 'photo.'))
               else 'privacy' if action.startswith(('token.', 'export.'))
               else 'auth')
    audit_rows += f'''
      <tr data-cat="{cat_key}" data-text="{(actor + ' ' + action + ' ' + target).lower()}" class="border-t border-line align-top">
        <td class="whitespace-nowrap py-3.5 pl-5 pr-4 font-mono text-xs text-muted">{when}</td>
        <td class="py-3.5 pr-4 text-xs font-medium">{actor}</td>
        <td class="py-3.5 pr-4 font-mono text-xs text-fg">{action}</td>
        <td class="py-3.5 pr-4 text-xs text-muted">{target}</td>
        <td class="py-3.5 pr-4">{result_chip(result)}</td>
        <td class="py-3.5 pr-5 font-mono text-[11px] leading-relaxed text-muted">{meta}</td>
      </tr>'''

M6_MAIN = f'''    <div class="mb-8 flex flex-col gap-4 sm:flex-row sm:items-end sm:justify-between">
      <div>
        <p class="font-mono text-xs uppercase tracking-[0.14em] text-muted">Administration</p>
        <h1 class="mt-1 font-display text-3xl font-bold tracking-tight">Audit log</h1>
        <p class="mt-2 max-w-2xl text-sm leading-relaxed text-muted">Security, moderation and account actions with actor, action, target, timestamp, result and metadata (§47). The log is append-only and access-controlled; it never contains passwords, tokens or unnecessary personal information (§47/§86).</p>
      </div>
      <span class="inline-flex w-fit items-center gap-1.5 rounded-full bg-[oklch(45%_0.15_265_/_0.12)] px-3 py-1.5 text-xs font-semibold text-[oklch(42%_0.13_265)]">{icon('shield', 'h-3.5 w-3.5')} Signed in as Admin</span>
    </div>

    <div class="mb-4 flex flex-col gap-3 lg:flex-row lg:items-center lg:justify-between">
      <div class="relative sm:w-80">
        {icon('search', 'pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted')}
        <input id="audit-search" type="search" placeholder="Search actor, action or target…" aria-label="Search audit events"
               class="w-full rounded-xl border border-line bg-surface py-2.5 pl-10 pr-4 text-sm placeholder:text-muted focus:border-accent-strong" />
      </div>
      <div id="audit-filters" class="flex flex-wrap items-center gap-2" role="group" aria-label="Filter by category">
        <button type="button" data-cat="all" class="cat-chip rounded-full border border-fg bg-fg px-3 py-1.5 text-xs font-medium text-bg">All</button>
        <button type="button" data-cat="auth" class="cat-chip rounded-full border border-line px-3 py-1.5 text-xs text-muted transition-colors hover:border-fg hover:text-fg">Auth</button>
        <button type="button" data-cat="account" class="cat-chip rounded-full border border-line px-3 py-1.5 text-xs text-muted transition-colors hover:border-fg hover:text-fg">Account &amp; roles</button>
        <button type="button" data-cat="moderation" class="cat-chip rounded-full border border-line px-3 py-1.5 text-xs text-muted transition-colors hover:border-fg hover:text-fg">Moderation</button>
        <button type="button" data-cat="privacy" class="cat-chip rounded-full border border-line px-3 py-1.5 text-xs text-muted transition-colors hover:border-fg hover:text-fg">Privacy</button>
      </div>
    </div>

    <div class="overflow-x-auto rounded-2xl border border-line bg-surface shadow-card">
      <table class="w-full min-w-[880px] text-left">
        <thead>
          <tr class="border-b border-line font-mono text-[11px] uppercase tracking-wide text-muted">
            <th scope="col" class="px-5 py-3 font-medium">Timestamp <span class="normal-case">(America/Sao_Paulo)</span></th>
            <th scope="col" class="py-3 pr-4 font-medium">Actor</th>
            <th scope="col" class="py-3 pr-4 font-medium">Action</th>
            <th scope="col" class="py-3 pr-4 font-medium">Target</th>
            <th scope="col" class="py-3 pr-4 font-medium">Result</th>
            <th scope="col" class="py-3 pr-5 font-medium">Metadata</th>
          </tr>
        </thead>
        <tbody id="audit-rows">{audit_rows}
        </tbody>
      </table>
      <p id="audit-empty" class="hidden px-5 py-10 text-center text-sm text-muted">No audit events match this search or filter.</p>
    </div>

    <section data-od-id="audit-notes" class="mt-6 rounded-2xl border border-line bg-surface p-6 shadow-card">
      <h2 class="font-display text-base font-bold">About this log</h2>
      <ul class="mt-3 space-y-2 text-sm leading-relaxed text-muted">
        <li class="flex gap-2.5">{icon('lock', 'h-4 w-4 mt-0.5 shrink-0 text-muted')} Audit records are themselves access-controlled (ADMIN only) and subject to retention policies (§47).</li>
        <li class="flex items-start gap-2.5">{icon('eye-off', 'h-4 w-4 mt-0.5 shrink-0 text-muted')}<span>Entries never contain passwords, tokens or unnecessary personal information — security events record actions and results, not secrets (§47).</span></li>
        <li class="flex items-start gap-2.5">{icon('history', 'h-4 w-4 mt-0.5 shrink-0 text-accent-strong')} The log is append-only: corrections appear as new entries; past records cannot be edited or deleted.</li>
      </ul>
    </section>'''

M6_JS = '''
    /* ---------- M6: audit filters ---------- */
    (function () {
      var search = document.getElementById('audit-search');
      var rows = Array.prototype.slice.call(document.querySelectorAll('#audit-rows tr'));
      var cat = 'all';
      function apply() {
        var q = (search.value || '').trim().toLowerCase();
        var visible = 0;
        rows.forEach(function (tr) {
          var show = (cat === 'all' || tr.getAttribute('data-cat') === cat) && (!q || tr.getAttribute('data-text').indexOf(q) !== -1);
          tr.classList.toggle('hidden', !show);
          if (show) visible++;
        });
        document.getElementById('audit-empty').classList.toggle('hidden', visible !== 0);
      }
      search.addEventListener('input', apply);
      document.getElementById('audit-filters').addEventListener('click', function (e) {
        var btn = e.target.closest('.cat-chip');
        if (!btn) return;
        cat = btn.getAttribute('data-cat');
        document.querySelectorAll('.cat-chip').forEach(function (b) {
          var on = b === btn;
          b.className = 'cat-chip rounded-full border px-3 py-1.5 text-xs ' +
            (on ? 'border-fg bg-fg font-medium text-bg' : 'border-line text-muted transition-colors hover:border-fg hover:text-fg');
        });
        apply();
      });
    })();'''

page('m5-users.html', 'User management', 'Admin-only user list with account states and audited role assignment.', 'M5 User management', 'm5-users.html', M5_MAIN, M5_JS)
page('m6-audit.html', 'Audit log', 'Filterable audit events for security, account and moderation actions.', 'M6 Audit log', 'm6-audit.html', M6_MAIN, M6_JS)

# ================= E1 / E2 — Error pages =================
def error_page(fname, title, tag, main):
    html = f'''<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>{title} — BikeNest</title>
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
      h1, h2 {{ text-wrap: balance; }}
      :focus-visible {{ outline: 2px solid oklch(45% 0.11 170); outline-offset: 2px; border-radius: 4px; }}
    }}
  </style>
</head>
<body class="flex min-h-screen flex-col bg-bg font-body text-fg">

  <header data-od-id="topnav-error" class="border-b border-line">
    <div class="mx-auto flex h-16 max-w-6xl items-center justify-between gap-6 px-5 lg:px-8">
      <a href="p1-landing.html" class="flex items-center gap-2.5" aria-label="BikeNest — home">
        <span class="grid h-9 w-9 place-items-center rounded-xl bg-accent text-white">{icon('bike', 'h-5 w-5')}</span>
        <span class="font-display text-lg font-bold tracking-tight">BikeNest</span>
      </a>
      <div class="flex items-center rounded-lg border border-line p-0.5 font-mono text-xs" role="group" aria-label="Language">
        <a href="#" lang="pt-BR" class="rounded-md px-2 py-1 text-muted hover:text-fg" aria-label="Português (Brasil)">PT</a>
        <a href="#" lang="en" aria-current="true" class="rounded-md bg-fg px-2 py-1 font-medium text-bg" aria-label="English">EN</a>
      </div>
    </div>
  </header>

  <main id="content" class="flex flex-1 items-center justify-center px-5 py-16">
{main}
  </main>

  <footer data-od-id="footer" class="border-t border-line">
    <div class="mx-auto flex max-w-6xl flex-wrap items-center justify-between gap-3 px-5 py-6 lg:px-8">
      <p class="text-xs text-muted">© 2026 BikeNest</p>
      <p class="font-mono text-xs text-muted">Prototype · {tag}</p>
    </div>
  </footer>

  <script>
{SHELL_JS}
  </script>

</body>
</html>
'''
    open(fname, 'w').write(html)
    print('wrote', fname, len(html.splitlines()), 'lines')

E1_MAIN = f'''    <div class="w-full max-w-lg text-center">
      <span class="mx-auto grid h-16 w-16 place-items-center rounded-2xl bg-[oklch(56%_0.12_170_/_0.10)] text-accent-strong">{icon('compass', 'h-8 w-8')}</span>
      <p class="mt-6 font-mono text-sm uppercase tracking-[0.2em] text-muted">Error 404</p>
      <h1 class="mt-2 font-display text-3xl font-bold tracking-tight sm:text-4xl">This page went off route.</h1>
      <p class="mt-3 text-sm leading-relaxed text-muted">The link may be outdated, or the address was typed wrong. Try searching for parking near where you're headed.</p>

      <form data-od-id="error-search" action="p2-search.html" method="get" class="mx-auto mt-6 flex max-w-md items-center gap-2" role="search">
        <div class="relative flex-1">
          {icon('search', 'pointer-events-none absolute left-3.5 top-1/2 h-4 w-4 -translate-y-1/2 text-muted')}
          <input type="search" name="q" placeholder="Search a destination…" aria-label="Search parking near a destination"
                 class="w-full rounded-xl border border-line bg-surface py-2.5 pl-10 pr-4 text-sm placeholder:text-muted focus:border-accent-strong" />
        </div>
        <button type="submit" class="rounded-xl bg-accent-strong px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-accent">Search</button>
      </form>

      <div class="mt-8 flex flex-wrap items-center justify-center gap-x-6 gap-y-2 text-sm">
        <a href="p1-landing.html" class="font-medium text-accent-strong underline-offset-4 hover:underline">Go to the home page</a>
        <a href="p2-search.html" class="text-muted underline-offset-4 hover:text-fg hover:underline">Browse parking spots</a>
        <a href="p7-about.html" class="text-muted underline-offset-4 hover:text-fg hover:underline">How BikeNest works</a>
      </div>
    </div>'''

E2_MAIN = f'''    <!-- htmx-4 swap fragment: everything between FRAGMENT-START / FRAGMENT-END is safe to swap into any target (§116.6, §85) -->
    <!-- fragment-start -->
    <div class="w-full max-w-md rounded-2xl border border-line bg-surface p-8 text-center shadow-card" data-od-id="error-card" role="alert">
      <span class="mx-auto grid h-14 w-14 place-items-center rounded-2xl bg-danger-soft text-danger">{icon('triangle-alert', 'h-7 w-7')}</span>
      <h1 class="mt-5 font-display text-2xl font-bold tracking-tight">Something went wrong on our side.</h1>
      <p class="mt-3 text-sm leading-relaxed text-muted">Your request didn't go through — it's not something you did. Nothing was saved; please try again. If it keeps happening, try again in a few minutes.</p>
      <div class="mt-6 flex flex-wrap items-center justify-center gap-3">
        <button type="button" onclick="location.reload()" class="rounded-xl bg-accent-strong px-4 py-2.5 text-sm font-semibold text-white transition-colors hover:bg-accent">Try again</button>
        <a href="p1-landing.html" class="rounded-xl border border-line px-4 py-2.5 text-sm font-medium text-fg transition-colors hover:bg-[oklch(20%_0.02_240_/_0.05)]">Back to home</a>
      </div>
      <p class="mt-6 border-t border-line pt-4 font-mono text-[11px] text-muted">Reference: BN-4F2K9 · share this code if you contact support</p>
    </div>
    <!-- fragment-end -->'''

error_page('e1-not-found.html', 'Page not found', 'E1 Not found (404)', E1_MAIN)
error_page('e2-error.html', 'Server error', 'E2 Error (5xx)', E2_MAIN)

# ================= Wire M1 → admin entry point =================
m1 = open('m1-moderation.html').read()
anchor = '    <section data-od-id="recent-activity"'
admin_strip = f'''    <section data-od-id="administration" class="mt-6">
      <div class="rounded-2xl border border-line bg-surface p-6 shadow-card">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <h2 class="font-display text-lg font-bold">Administration</h2>
          <span class="inline-flex items-center gap-1.5 rounded-full bg-[oklch(20%_0.02_240_/_0.05)] px-2.5 py-1 font-mono text-[11px] uppercase tracking-wide text-muted">{icon('lock', 'h-3 w-3')} Visible to ADMIN only</span>
        </div>
        <div class="mt-4 grid gap-4 sm:grid-cols-2">
          <a href="m5-users.html" class="group flex items-center justify-between rounded-xl border border-line p-4 transition-colors hover:bg-[oklch(20%_0.02_240_/_0.04)]">
            <span class="flex items-center gap-3">
              <span class="grid h-9 w-9 place-items-center rounded-lg bg-[oklch(45%_0.15_265_/_0.12)] text-[oklch(42%_0.13_265)]">{icon('users', 'h-4 w-4')}</span>
              <span><span class="block text-sm font-semibold">User management</span><span class="block text-xs text-muted">Account states, role grants and revocations (§19)</span></span>
            </span>
            {icon('arrow-right', 'h-4 w-4 text-muted transition-transform group-hover:translate-x-0.5')}
          </a>
          <a href="m6-audit.html" class="group flex items-center gap-3 rounded-xl border border-line p-4 transition-colors hover:bg-[oklch(20%_0.02_240_/_0.04)]">
            <span class="grid h-9 w-9 place-items-center rounded-lg bg-[oklch(56%_0.12_170_/_0.10)] text-accent-strong">{icon('history', 'h-4 w-4')}</span>
            <span class="flex-1"><span class="block text-sm font-semibold">Audit log</span><span class="block text-xs text-muted">Append-only trail of security and moderation events (§47)</span></span>
            {icon('arrow-right', 'h-4 w-4 text-muted transition-transform group-hover:translate-x-0.5')}
          </a>
        </div>
      </div>
    </section>

'''
assert anchor in open('m1-moderation.html').read(), 'M1 anchor missing'
s = open('m1-moderation.html').read().replace(anchor, admin_strip + anchor, 1)
open('m1-moderation.html', 'w').write(s)
print('wired admin strip into m1')

print('done')
