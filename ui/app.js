'use strict';

const { invoke } = window.__TAURI__.core;
const appWindow = () => window.__TAURI__.window.getCurrentWindow();
const $ = (id) => document.getElementById(id);
const SVGNS = 'http://www.w3.org/2000/svg';
const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches;

function h(tag, props, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(props || {})) {
    if (v == null || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (v === true) el.setAttribute(k, '');
    else el.setAttribute(k, v);
  }
  for (const kid of kids.flat()) {
    if (kid == null || kid === false) continue;
    el.append(kid.nodeType ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

function icon(name) {
  const s = document.createElementNS(SVGNS, 'svg');
  s.setAttribute('class', 'ico');
  s.setAttribute('aria-hidden', 'true');
  const u = document.createElementNS(SVGNS, 'use');
  u.setAttribute('href', '#i-' + name);
  s.append(u);
  return s;
}

function errBox(plain, raw, cls) {
  return h('div', { class: 'notice ' + (cls || 'bad'), role: 'alert' }, plain, raw ? h('span', { class: 'raw' }, raw) : null);
}

function toast(msg, kind) {
  const box = $(kind === 'err' ? 'toasts-err' : 'toasts');
  while (box.children.length >= 2) box.firstChild.remove();
  const el = h('div', { class: 'toast' + (kind === 'err' ? ' err' : '') }, msg);
  box.append(el);
  setTimeout(() => el.remove(), kind === 'err' ? 9000 : 5000);
}

function announce(msg) {
  const el = $('sr-status');
  if (el) el.textContent = msg;
}

function enqueuedToast(info) {
  if (info.state === 'failed') toast(info.error || `${info.name} could not start.`, 'err');
  else if (info.state === 'running') toast(`Started ${info.name}. Discord should pick it up in a few seconds.`);
  else toast(`Added ${info.name} to the queue. It starts when the game before it ends.`);
}

function startText(kind) {
  if (S.pending > 0) return 'Add to queue';
  return kind === 'steam' ? 'Create manifest and start' : 'Start fake process';
}
function startLabel(kind) { return h('span', { class: 'label', 'data-kind': kind }, startText(kind)); }
function refreshStartLabels() {
  document.querySelectorAll('.label[data-kind]').forEach((el) => { el.textContent = startText(el.dataset.kind); });
}

async function call(cmd, args) {
  try {
    return await invoke(cmd, args);
  } catch (e) {
    throw typeof e === 'string' ? e : (e && e.message) || String(e);
  }
}

async function withBusy(btn, fn, canRun) {
  btn.classList.add('busy');
  btn.setAttribute('aria-busy', 'true');
  btn.disabled = true;
  try {
    return await fn();
  } finally {
    btn.classList.remove('busy');
    btn.removeAttribute('aria-busy');
    btn.disabled = canRun ? !canRun() : false;
  }
}

function debounce(fn, ms) {
  let t = null;
  const d = (...a) => { clearTimeout(t); t = setTimeout(() => { t = null; fn(...a); }, ms); };
  d.cancel = () => { clearTimeout(t); t = null; };
  d.flush = (...a) => { d.cancel(); return fn(...a); };
  return d;
}

const S = { boot: null, pending: 0, list: [], prev: {}, view: 'discord', db: null, dbState: 'loading', dbError: '', onDbChange: null, leave: [] };
const V = {};

function onLeave(fn) { S.leave.push(fn); }

const NAV_TOP = [
  ['discord', 'Games'],
  ['manual', 'Manual'],
  ['steam', 'Steam quests'],
];
const NAV_BOTTOM = [
  ['settings', 'Settings'],
  ['about', 'About'],
];

const VIEW_ORDER = [...NAV_TOP, ...NAV_BOTTOM].map(([id]) => id);

function buildNav() {
  const make = ([id, label]) => {
    const key = 'Ctrl+' + (VIEW_ORDER.indexOf(id) + 1);
    return h('button', { type: 'button', 'data-view': id, title: `${label} (${key})`, 'aria-keyshortcuts': key, onclick: () => go(id) }, label);
  };
  $('nav-top').replaceChildren(...NAV_TOP.map(make));
  $('nav-bottom').replaceChildren(...NAV_BOTTOM.map(make));
}

function go(name) {
  S.leave.splice(0).forEach((fn) => fn());
  S.onDbChange = null;
  S.view = name;
  document.querySelectorAll('.nav button').forEach((b) => {
    if (b.dataset.view === name) b.setAttribute('aria-current', 'page');
    else b.removeAttribute('aria-current');
  });
  const root = $('view');
  root.replaceChildren();
  root.scrollTop = 0;
  $('foot').replaceChildren();
  $('foot').hidden = true;
  ({ discord: viewDiscord, manual: viewManual, steam: viewSteam, settings: viewSettings, about: viewAbout })[name](root);
  const target = root.querySelector('[data-autofocus]') || root.querySelector('h1');
  if (target) { target.setAttribute('tabindex', target.tagName === 'H1' ? '-1' : target.getAttribute('tabindex') || '0'); target.focus({ preventScroll: true }); }
}

function head(title, lede) {
  return h('header', null, h('h1', null, title), lede ? h('p', { class: 'lede' }, lede) : null);
}

let rowSeq = 0;
function expandRow(list, headKids, buildBody, opts) {
  const id = 'more-' + (++rowSeq);
  const headBtn = h('button', { type: 'button', class: 'row-head', 'aria-expanded': 'false', 'aria-controls': id }, headKids);
  const host = h('div');
  const more = h('div', { class: 'row-more', id, inert: true }, h('div', null, host));
  const row = h('div', { class: 'row', role: 'listitem', 'data-open': 'false' }, headBtn, more);
  let built = false;
  const set = (open, animate) => {
    if (open && !built) { built = true; host.replaceChildren(buildBody()); }
    row.dataset.open = String(open);
    headBtn.setAttribute('aria-expanded', String(open));
    if (open) more.removeAttribute('inert'); else more.setAttribute('inert', '');
    if (open && animate) setTimeout(() => row.scrollIntoView({ block: 'nearest', behavior: reducedMotion ? 'auto' : 'smooth' }), 280);
    if (opts && opts.onChange) opts.onChange(open);
  };
  row._set = set;
  headBtn.addEventListener('click', () => {
    const open = row.dataset.open !== 'true';
    if (open) list.querySelectorAll('.row[data-open="true"]').forEach((r) => r._set(false, false));
    set(open, true);
  });
  if (opts && opts.startOpen) set(true, false);
  return row;
}

async function loadDb(refresh) {
  S.dbState = 'loading';
  paintDbChip();
  try {
    S.db = await call('load_database', { refresh: !!refresh });
    S.dbState = 'ok';
    S.dbError = '';
  } catch (e) {
    S.dbState = 'bad';
    S.dbError = e;
  }
  paintDbChip();
  if (S.onDbChange) S.onDbChange();
}

function paintDbChip() {
  const el = document.getElementById('dbchip');
  if (!el) return;
  if (!el._text) {
    el._text = document.createTextNode('');
    el._btn = h('button', { type: 'button', title: 'Reload', onclick: () => { if (S.dbState !== 'loading') loadDb(true); } }, icon('refresh'));
    el.replaceChildren(h('span', { class: 'dot' }), el._text, el._btn);
  }
  if (S.dbState === 'loading') {
    el.className = 'db-status';
    el._text.data = 'Loading the games list…';
    el._btn.setAttribute('aria-disabled', 'true');
    el._btn.setAttribute('aria-label', 'Reload the games list, available when loading ends');
  } else if (S.dbState === 'ok') {
    el.className = 'db-status ok';
    el._text.data = `${S.db.source.includes('Discord') ? 'Discord' : 'Backup list'} · ${S.db.count.toLocaleString('en-US')} games`;
    el._btn.removeAttribute('aria-disabled');
    el._btn.setAttribute('aria-label', 'Reload the games list');
  } else {
    el.className = 'db-status bad';
    el._text.data = 'Games list unavailable';
    el._btn.removeAttribute('aria-disabled');
    el._btn.setAttribute('aria-label', 'Try loading the games list again');
  }
}

function viewDiscord(root) {
  const s = (V.discord ||= { q: '', lastQ: '', results: null, loading: false, error: '', open: null, seq: 0, exe: {}, openFirst: false });
  const input = h('input', { type: 'search', id: 'q', 'data-autofocus': true, placeholder: 'PUBG, League, Minecraft…', value: s.q, autocomplete: 'off', spellcheck: 'false', 'aria-label': 'Search games' });
  const list = h('div');
  const dbchip = h('div', { id: 'dbchip' });

  root.append(
    head('Find a game', 'Search Discord’s games list by name or abbreviation.'),
    h('div', { class: 'search-panel' }, h('div', { class: 'searchbar' }, icon('search'), input), dbchip),
    list,
  );
  paintDbChip();

  const run = async () => {
    const my = ++s.seq;
    const q = s.q.trim();
    s.error = '';
    if (!q) { s.results = null; s.loading = false; paint(); return; }
    if (S.dbState === 'loading') { s.loading = true; paint(); return; }
    s.loading = true; paint();
    try {
      const r = await call('db_search', { query: q });
      if (my !== s.seq) return;
      s.results = r;
      if (q !== s.lastQ) s.open = null;
      s.lastQ = q;
    } catch (e) {
      if (my !== s.seq) return;
      s.results = []; s.error = e;
    }
    s.loading = false; paint();
  };
  const runSoon = debounce(run, 180);
  onLeave(runSoon.cancel);
  S.onDbChange = () => { if (s.q.trim() && S.dbState === 'ok') run(); else { s.loading = false; paint(); } };
  input.addEventListener('input', () => { s.q = input.value; s.openFirst = false; runSoon(); });
  input.addEventListener('keydown', async (e) => {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    s.openFirst = true;
    await runSoon.flush();
  });

  function paint() {
    if (!list.isConnected) return;
    if (s.loading) {
      list.replaceChildren(h('div', { class: 'list', 'aria-hidden': 'true' }, h('div', { class: 'skeleton' }), h('div', { class: 'skeleton' }), h('div', { class: 'skeleton' })));
    } else if (S.dbState === 'bad') {
      list.replaceChildren(errBox('Couldn’t load the games list. Check your connection and reload it, or use Manual to type a process name.', S.dbError));
    } else if (s.error) {
      list.replaceChildren(errBox('The search failed. Try again in a moment.', s.error));
    } else if (s.results === null) {
      list.replaceChildren(h('div', { class: 'suggestions' }, h('p', { class: 'hint' }, 'Try a search'),
        h('div', { class: 'chips' }, ['PUBG', 'League', 'Minecraft', 'Fortnite'].map((x) =>
          h('button', { type: 'button', class: 'chip pick', onclick: () => { input.value = x; s.q = x; run(); } }, x)))));
    } else if (!s.results.length) {
      list.replaceChildren(h('div', { class: 'empty' }, `No games match “${s.q.trim()}”. Try a shorter name or an abbreviation, or use Manual to type the process name.`));
    } else {
      announce(`${s.results.length} games found.`);
      list.replaceChildren(...[
        h('div', { class: 'list', role: 'list' }, s.results.map((g) => gameRow(g, list))),
        s.results.length >= 20 ? h('p', { class: 'hint', style: 'margin-top:10px' }, 'Showing the first 20 matches. Type more of the name to narrow it down.') : null,
      ].filter(Boolean));
      if (s.openFirst) {
        s.openFirst = false;
        const first = list.querySelector('.row-head');
        if (first && first.getAttribute('aria-expanded') !== 'true') first.click();
      }
    }
  }

  function gameRow(g, host) {
    const exeOf = () => s.exe[g.id] ?? g.primary ?? g.executables[0] ?? '';
    const buildBody = () => {
      const pathEl = h('div', { class: 'path mono' }, '—');
      let pseq = 0;
      const refreshPath = async () => {
        const my = ++pseq;
        const exe = exeOf();
        if (!exe.trim()) { pathEl.textContent = '—'; return; }
        try { const p = await call('preview_target', { exe }); if (my === pseq) pathEl.textContent = p; } catch { if (my === pseq) pathEl.textContent = ''; }
      };
      const start = h('button', { type: 'button', class: 'btn', disabled: !exeOf().trim(), onclick: (e) => withBusy(e.currentTarget, async () => {
        const exe = exeOf();
        try {
          enqueuedToast(await call('spoof', { req: { name: g.name, exe, kind: 'discord' } }));
          poll();
        } catch (err) { toast(err, 'err'); }
      }, () => exeOf().trim()) }, startLabel('discord'));
      const chipBox = h('div', { class: 'chips' });
      const paintChips = () => chipBox.replaceChildren(...g.executables.slice(0, 12).map((x) =>
        h('button', { type: 'button', class: 'chip pick', 'aria-pressed': String(x === exeOf()), onclick: () => { s.exe[g.id] = x; paintChips(); refreshPath(); } }, x)));
      let pick;
      if (g.executables.length > 1) { paintChips(); pick = h('div', { class: 'field' }, h('span', { class: 'label' }, 'Process name'), chipBox); }
      else if (g.executables.length === 1) pick = h('div', { class: 'field' }, h('span', { class: 'label' }, 'Process name'), h('div', { class: 'mono' }, exeOf()));
      else {
        const custom = h('input', { type: 'text', class: 'mono', placeholder: 'TslGame.exe', value: s.exe[g.id] || '', 'aria-label': 'Process name', oninput: (e) => { s.exe[g.id] = e.target.value; start.disabled = !e.target.value.trim(); refreshPath(); } });
        pick = h('div', { class: 'field' }, errBox('Discord lists no Windows executable for this game. Enter the process name yourself.', null, ''), custom);
      }
      refreshPath();
      return h('div', { class: 'row-body' }, pick, h('div', { class: 'field' }, h('span', { class: 'label' }, 'Created at'), pathEl), h('div', { class: 'actions' }, start));
    };
    const aliasText = g.aliases.length ? 'Also known as ' + g.aliases.slice(0, 3).join(', ') + (g.aliases.length > 3 ? ` +${g.aliases.length - 3}` : '') : 'ID ' + g.id;
    return expandRow(host, [
      h('span', { class: 'row-name' }, g.name || 'Unnamed game'),
      h('span', { class: 'row-meta' }, aliasText),
      h('span', { class: 'row-tag' }, g.primary || ''),
    ], buildBody, { startOpen: s.open === g.id, onChange: (open) => { if (open) s.open = g.id; else if (s.open === g.id) s.open = null; } });
  }

  if (s.results) paint(); else if (s.q.trim()) run(); else paint();
}

function viewManual(root) {
  const s = (V.manual ||= { exe: '' });
  const input = h('input', { type: 'text', class: 'mono', id: 'exe', 'data-autofocus': true, placeholder: 'TslGame.exe', value: s.exe, autocomplete: 'off', spellcheck: 'false' });
  const pathEl = h('div', { class: 'path mono' }, '—');
  const start = h('button', { type: 'button', class: 'btn', disabled: !s.exe.trim(), onclick: (e) => withBusy(e.currentTarget, async () => {
    try {
      enqueuedToast(await call('spoof', { req: { name: s.exe.trim(), exe: s.exe, kind: 'manual' } }));
      poll();
    } catch (err) { toast(err, 'err'); }
  }, () => s.exe.trim()) }, startLabel('manual'));

  let pseq = 0;
  const refresh = debounce(async () => {
    const my = ++pseq;
    if (!s.exe.trim()) { pathEl.textContent = '—'; return; }
    try { const p = await call('preview_target', { exe: s.exe }); if (my === pseq) pathEl.textContent = p; } catch {                                  }
  }, 150);
  onLeave(refresh.cancel);
  const setExe = (v) => { s.exe = v; start.disabled = !v.trim(); refresh(); };
  input.addEventListener('input', () => setExe(input.value));
  input.addEventListener('keydown', (e) => { if (e.key === 'Enter' && !start.disabled) { e.preventDefault(); start.click(); } });

  root.append(
    head('Type a process name', 'Enter the exact executable name Discord expects.'),
    h('div', { class: 'field' }, h('label', { for: 'exe' }, 'Executable name'), input),
    h('div', { class: 'chips' }, ['TslGame.exe', 'League of Legends.exe', 'Overwatch.exe'].map((x) =>
      h('button', { type: 'button', class: 'chip pick', onclick: () => { input.value = x; setExe(x); } }, x))),
    h('div', { class: 'field' }, h('span', { class: 'label' }, 'Created at'), pathEl),
    h('div', { class: 'actions' }, start),
  );
  refresh();
}

function viewSteam(root) {
  const s = (V.steam ||= { path: (S.boot && S.boot.steam_path) || '', q: '', lastQ: '', results: null, loading: false, error: '', seq: 0, open: null, forms: {}, openFirst: false });
  const pathInput = h('input', { type: 'text', class: 'mono', id: 'steam-path', value: s.path, 'aria-describedby': 'steam-hint', spellcheck: 'false' });
  const q = h('input', { type: 'search', 'data-autofocus': true, placeholder: 'Search the Steam store', value: s.q, autocomplete: 'off', spellcheck: 'false', 'aria-label': 'Search Steam games' });
  const list = h('div');
  const validators = new Set();
  const revalidateAll = () => validators.forEach((fn) => fn());
  pathInput.addEventListener('input', () => { s.path = pathInput.value; revalidateAll(); });
  onLeave(() => validators.clear());

  const browse = h('button', { type: 'button', class: 'btn quiet', onclick: async () => {
    const dir = await pickFolder(s.path);
    if (dir) { s.path = dir; pathInput.value = dir; revalidateAll(); }
  } }, 'Browse');

  root.append(
    head('Steam quests', 'Some quests also check that Steam is “downloading” the game. This creates the manifest Steam writes for a download in progress, and places the fake game next to it.'),
    h('div', { class: 'field' }, h('label', { for: 'steam-path' }, 'Steam folder'),
      h('div', { class: 'inline-input' }, pathInput, browse),
      h('span', { class: 'hint', id: 'steam-hint' }, s.path ? 'Found automatically. Change it if your library lives elsewhere.' : 'Steam wasn’t found automatically. Choose the folder that contains steam.exe.')),
    h('div', { class: 'field' }, h('span', { class: 'label' }, 'Game'), h('div', { class: 'searchbar' }, icon('search'), q),
      h('span', { class: 'hint' }, 'Demos and full games have different AppIDs. If the quest targets a demo, search for “Name Demo”.')),
    list,
  );

  const run = async () => {
    const my = ++s.seq;
    const text = q.value.trim();
    s.error = '';
    if (!text) { s.results = null; s.loading = false; paint(); return; }
    s.loading = true; paint();
    try {
      const r = await call('steam_search', { query: text });
      if (my !== s.seq) return;
      s.results = r;
      if (text !== s.lastQ) s.open = null;
      s.lastQ = text;
    } catch (e) { if (my !== s.seq) return; s.results = []; s.error = e; }
    s.loading = false; paint();
  };
  const runSoon = debounce(run, 300);
  onLeave(runSoon.cancel);
  q.addEventListener('input', () => { s.q = q.value; s.openFirst = false; runSoon(); });
  q.addEventListener('keydown', async (e) => {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    s.openFirst = true;
    await runSoon.flush();
  });

  function paint() {
    if (!list.isConnected) return;
    if (s.loading) list.replaceChildren(h('div', { class: 'list', 'aria-hidden': 'true' }, h('div', { class: 'skeleton' }), h('div', { class: 'skeleton' })));
    else if (s.error) list.replaceChildren(errBox('Steam search failed. Check your connection and try again.', s.error));
    else if (s.results === null) list.replaceChildren(h('div', { class: 'empty' }, 'Search for a game to see its AppID and details.'));
    else if (!s.results.length) list.replaceChildren(h('div', { class: 'empty' }, `No Steam results for “${s.q.trim()}”. Try another spelling.`));
    else {
      announce(`${s.results.length} Steam results found.`);
      list.replaceChildren(h('div', { class: 'list', role: 'list' }, s.results.map((i) => steamRow(i, list))));
      if (s.openFirst) {
        s.openFirst = false;
        const first = list.querySelector('.row-head');
        if (first && first.getAttribute('aria-expanded') !== 'true') first.click();
      }
    }
  }

  function steamRow(item, host) {
    const f = (s.forms[item.id] ||= { ready: false, name: item.name, installdir: '', executable: '', depot_id: null, note: null });
    const bodyHost = h('div', { class: 'row-body' });

    async function load() {
      if (f.ready) { renderForm(); return; }
      bodyHost.replaceChildren(h('div', { class: 'hint' }, 'Fetching details from SteamCMD…'));
      try {
        const info = await call('steam_info', { appid: item.id });
        for (const k of ['installdir', 'executable']) if (!String(f[k] || '').trim()) f[k] = info[k];
        if (!String(f.name || '').trim()) f.name = info.name;
        f.depot_id = info.depot_id;
        f.note = null;
        f.ready = true;
      } catch {
        f.note = 'Couldn’t fetch details from SteamCMD. Try again, or enter the install folder and the executable yourself. Both are inside steamapps/common.';
      }
      if (bodyHost.isConnected) renderForm();
    }

    function renderForm() {
      const mp = h('div', { class: 'path mono' }, '—');
      const ep = h('div', { class: 'path mono' }, '—');
      const req = () => ({ appid: item.id, name: f.name, installdir: f.installdir, executable: f.executable, depot_id: f.depot_id, steam_path: s.path });
      let pseq = 0;
      const preview = debounce(async () => {
        const my = ++pseq;
        try { const p = await call('steam_preview', { req: req() }); if (my === pseq) { mp.textContent = p.manifest; ep.textContent = p.exe; } } catch {                   }
      }, 150);
      onLeave(preview.cancel);
      const start = h('button', { type: 'button', class: 'btn', onclick: (e) => withBusy(e.currentTarget, async () => {
        try {
          enqueuedToast(await call('steam_spoof', { req: req() }));
          poll();
        } catch (err) { toast(err, 'err'); }
      }, () => valid()) }, startLabel('steam'));
      const valid = () => [s.path, f.name, f.installdir, f.executable].every((v) => String(v || '').trim());
      const sync = () => { if (!start.classList.contains('busy')) start.disabled = !valid(); };
      validators.add(sync);
      const field = (label, key, mono) => h('div', { class: 'field' }, h('label', null, label),
        h('input', { type: 'text', class: mono ? 'mono' : '', value: f[key] || '', spellcheck: 'false', oninput: (e) => { f[key] = e.target.value; sync(); preview(); } }));
      bodyHost.replaceChildren(...[
        f.note ? h('div', { class: 'notice', role: 'alert' }, f.note, h('div', { style: 'margin-top:10px' }, h('button', { type: 'button', class: 'btn quiet small', onclick: () => { f.note = null; load(); } }, 'Try again'))) : null,
        h('div', { class: 'grid2' }, field('Name', 'name'), field('Install folder', 'installdir')),
        field('Executable', 'executable', true),
        h('div', { class: 'field' }, h('span', { class: 'label' }, 'Manifest'), mp),
        h('div', { class: 'field' }, h('span', { class: 'label' }, 'Fake game'), ep),
        h('div', { class: 'actions' }, start, f.depot_id ? h('span', { class: 'hint' }, 'Depot ' + f.depot_id) : null),
      ].filter(Boolean));
      sync();
      preview();
    }

    return expandRow(host, [
      h('span', { class: 'row-name' }, item.name), h('span', { class: 'row-meta' }, 'AppID ' + item.id),
    ], () => { load(); return bodyHost; }, { startOpen: s.open === item.id, onChange: (open) => { if (open) s.open = item.id; else if (s.open === item.id) s.open = null; } });
  }
  if (s.results) paint(); else if (s.q.trim()) run(); else paint();
}

async function pickFolder(current) {
  try {
    const r = await window.__TAURI__.dialog.open({ directory: true, multiple: false, defaultPath: current || undefined });
    return typeof r === 'string' ? r : null;
  } catch (e) {
    toast('Couldn’t open the folder picker. Type the path instead.', 'err');
    return null;
  }
}

function viewSettings(root) {
  const saved = S.boot.settings;
  const cur = { ...saved };
  const folder = h('input', { type: 'text', class: 'mono', id: 'folder', value: cur.CHOSEN_FOLDER, spellcheck: 'false' });
  const sub = h('input', { type: 'text', class: 'mono', id: 'sub', value: cur.FAKE_EXE_DIR, spellcheck: 'false' });
  const num = h('input', { type: 'number', id: 'mins', min: '1', max: '1440', value: cur.TIMER_MINUTES });
  const rng = h('input', { type: 'range', class: 'range', min: '1', max: '120', value: Math.min(cur.TIMER_MINUTES, 120), 'aria-label': 'Timer minutes slider' });
  const sw = h('button', { type: 'button', class: 'switch', role: 'switch', id: 'auto', 'aria-checked': String(cur.AUTO_DELETE), 'aria-labelledby': 'auto-label' });
  const where = h('div', { class: 'path mono' });
  const save = h('button', { type: 'button', class: 'btn' }, 'Save settings');
  const revert = h('button', { type: 'button', class: 'btn quiet' }, 'Revert');

  const minsHint = h('span', { class: 'hint', id: 'mins-hint' }, '');
  num.setAttribute('aria-describedby', 'mins-hint');
  const minsOk = () => String(num.value).trim() !== '' && Number.isInteger(Number(cur.TIMER_MINUTES)) && Number(cur.TIMER_MINUTES) >= 1 && Number(cur.TIMER_MINUTES) <= 1440;
  const valid = () => minsOk() && String(cur.CHOSEN_FOLDER).trim() && String(cur.FAKE_EXE_DIR).trim();
  const dirty = () => cur.CHOSEN_FOLDER !== saved.CHOSEN_FOLDER || cur.FAKE_EXE_DIR !== saved.FAKE_EXE_DIR || Number(cur.TIMER_MINUTES) !== saved.TIMER_MINUTES || cur.AUTO_DELETE !== saved.AUTO_DELETE;
  const sync = () => {
    where.textContent = `${cur.CHOSEN_FOLDER}\\${cur.FAKE_EXE_DIR}`.replace(/[\\/]+/g, '\\');
    save.disabled = !dirty() || !valid();
    revert.disabled = !dirty();
    minsHint.className = minsOk() ? 'hint' : 'hint bad';
    minsHint.textContent = minsOk() ? 'Each new game runs for this duration.' : 'Enter a whole number from 1 to 1440.';
  };
  folder.addEventListener('input', () => { cur.CHOSEN_FOLDER = folder.value; sync(); });
  sub.addEventListener('input', () => { cur.FAKE_EXE_DIR = sub.value; sync(); });
  num.addEventListener('input', () => { cur.TIMER_MINUTES = Number(num.value); rng.value = Math.min(cur.TIMER_MINUTES, 120); sync(); });
  rng.addEventListener('input', () => { cur.TIMER_MINUTES = Number(rng.value); num.value = rng.value; sync(); });
  sw.addEventListener('click', () => { cur.AUTO_DELETE = !cur.AUTO_DELETE; sw.setAttribute('aria-checked', String(cur.AUTO_DELETE)); sync(); });
  revert.addEventListener('click', () => go('settings'));
  save.addEventListener('click', () => withBusy(save, async () => {
    try {
      S.boot.settings = await call('settings_save', { settings: { ...cur, TIMER_MINUTES: Number(cur.TIMER_MINUTES) } });
      toast('Settings saved. They apply to games you start from now on.');
      const top = $('view').scrollTop;
      go('settings');
      $('view').scrollTop = top;
    } catch (e) { toast(e, 'err'); }
  }));

  root.append(
    head('Settings', 'These are baked into every fake game when you start it, so changes apply to new games only.'),
    h('div', { class: 'stack' },
      h('div', { class: 'panel' },
        h('div', { class: 'field' }, h('label', { for: 'folder' }, 'Folder for fake games'),
          h('div', { class: 'inline-input' }, folder, h('button', { type: 'button', class: 'btn quiet', onclick: async () => {
            const d = await pickFolder(cur.CHOSEN_FOLDER);
            if (d) { cur.CHOSEN_FOLDER = d; folder.value = d; sync(); }
          } }, 'Browse'))),
        h('div', { class: 'field' }, h('label', { for: 'sub' }, 'Subfolder name'), sub),
        h('div', { class: 'field' }, h('span', { class: 'label' }, 'Fake games are created in'), where),
      ),
      h('div', { class: 'panel' },
        h('div', { class: 'field' }, h('label', { for: 'mins' }, 'Timer length in minutes'), num, rng,
          minsHint),
        h('div', { class: 'switch-row' },
          h('div', null, h('div', { id: 'auto-label', style: 'font-weight:600' }, 'Delete fake games when done'),
            h('div', { class: 'hint' }, 'When the timer ends, the game deletes itself and any empty folders it made. Closing orbshacker stops and deletes the running one too.')),
          sw),
      ),
    ),
  );
  const foot = $('foot');
  foot.replaceChildren(h('div', { class: 'actions' }, save, revert, h('span', { class: 'hint', style: 'overflow:hidden;text-overflow:ellipsis;white-space:nowrap;min-width:0' }, S.boot.settings_file)));
  foot.hidden = false;
  sync();
}

function viewAbout(root) {
  const b = S.boot;
  root.append(
    head('About orbshacker', 'Creates fake game processes so Discord’s quest detection sees a game running.'),
    h('dl', { class: 'kv' },
      h('dt', null, 'Version'), h('dd', null, b.version),
      h('dt', null, 'Desktop app developer'), h('dd', null, 'Proximax1'),
      h('dt', null, 'Original project developers'), h('dd', null, b.developer)),
    h('div', { class: 'prose' },
      h('p', null, 'Discord has to be running for any of this to work. It scans your process list, finds a process with the expected name, and treats the game as running. The fake process must stay open for as long as the quest needs.'),
      h('p', null, 'Steam quests add a manifest in your Steam library, the file Steam writes while a download is in progress. orbshacker never overwrites a real game’s files or manifest. If one is in the way, it stops and tells you.'),
      h('p', null, 'Only one fake game is active at a time. Start more and they wait in the queue, each running for the timer length in turn.'),
    ),
    h('div', { class: 'notice' }, 'Educational use only. You are responsible for following Discord’s terms of service. The developers accept no responsibility for what happens to your account.'),
  );
}

const KIND = { discord: 'Discord', manual: 'Manual', steam: 'Steam' };

function clock(secs) {
  const hh = Math.floor(secs / 3600), mm = Math.floor((secs % 3600) / 60), ss = secs % 60;
  const p = (n) => String(n).padStart(2, '0');
  return hh > 0 ? `${hh}:${p(mm)}:${p(ss)}` : `${p(mm)}:${p(ss)}`;
}

function stateText(x) {
  switch (x.state) {
    case 'running': return 'Running';
    case 'queued': return x.queue_pos === 1 ? 'Next up' : `In line, number ${x.queue_pos}`;
    case 'done': return x.files_present ? 'Done, files kept' : 'Done';
    case 'stopped': return x.files_present ? 'Stopped, files kept' : 'Stopped';
    case 'exited': return x.files_present ? 'Ended early, files kept' : 'Ended early';
    default: return 'Could not start';
  }
}

function sessionEl(x) {
  return h('div', { class: 'sess', 'data-id': x.id },
    h('div', { class: 'sess-info' }, h('div', { class: 'sess-name' }, x.name), h('div', { class: 'sess-state' })),
    h('div', { class: 'sess-err' }),
    h('div', { class: 'sess-clock', hidden: true },
      h('span', { class: 'sr' }, 'Time remaining'),
      h('span', { class: 'time', role: 'timer' })),
    h('div', { class: 'sess-actions' }),
  );
}

function paintSession(el, x) {
  const running = x.state === 'running';
  const timer = el.querySelector('.time');
  const time = running ? clock(x.remaining_secs) : '';
  if (timer.textContent !== time) timer.textContent = time;
  el.querySelector('.sess-clock').hidden = !running;
  const status = el.querySelector('.sess-state');
  const subtitle = `${stateText(x)} · ${KIND[x.kind]}`;
  if (status.textContent !== subtitle) status.textContent = subtitle;
  const key = x.state + (x.files_present ? '1' : '0');
  if (el.dataset.key === key) return;
  el.dataset.key = key;
  el.dataset.state = x.state;
  el.querySelector('.sess-err').textContent = x.error || '';
  const acts = el.querySelector('.sess-actions');
  acts.replaceChildren();
  const act = (label, aria, danger, fn) => h('button', { type: 'button', class: 'btn quiet small' + (danger ? ' danger' : ''), 'aria-label': aria, onclick: (e) => withBusy(e.currentTarget, async () => {
    try { await fn(); } catch (err) { toast(err, 'err'); }
    await refreshSessions();
    if (document.activeElement === document.body) $('dock-title').focus();
  }) }, label);
  if (x.state === 'running') {
    acts.append(act('Stop', `Stop ${x.name}`, true, async () => {
      const r = await call('session_stop', { id: x.id });
      if (r.failures.length) toast(`Stopped ${x.name}. These files are locked and stayed: ${r.failures.join(', ')}.`, 'err');
      else toast(r.deleted ? `Stopped ${x.name} and deleted its files.` : `Stopped ${x.name}. Its files are still there.`);
    }));
  } else if (x.state === 'queued') {
    acts.append(act('Remove', `Remove ${x.name} from the queue`, false, async () => {
      await call('session_cancel', { id: x.id });
      toast(`Removed ${x.name} from the queue.`);
    }));
  } else {
    if (x.files_present) {
      acts.append(act('Delete files', `Delete the files of ${x.name}`, false, async () => {
        const r = await call('session_clean', { id: x.id });
        if (r.failures.length) toast(`These files are locked and stayed: ${r.failures.join(', ')}.`, 'err');
        else toast(`Deleted the files of ${x.name}.`);
      }));
    } else {
      acts.append(act('Remove', `Remove ${x.name} from the list`, false, () => call('session_dismiss', { id: x.id })));
    }
  }
}

async function refreshSessions() {
  while (polling) await new Promise((r) => setTimeout(r, 30));
  await poll();
}

let polling = false;
let pollAgain = false;
let pollFails = 0;
async function poll() {
  if (!S.boot) return;
  if (polling) { pollAgain = true; return; }
  polling = true;
  try {
    const list = await call('sessions_list');
    pollFails = 0;
    S.list = list;
    for (const x of list) {
      const was = S.prev[x.id];
      if (was && was !== x.state) {
        if (x.state === 'running') announce(`${x.name} started.`);
        else if (x.state === 'done') announce(`${x.name} finished.`);
        else if (x.state === 'exited') announce(`${x.name} ended early.`);
        else if (x.state === 'failed') toast(x.error || `${x.name} could not start.`, 'err');
      }
      S.prev[x.id] = x.state;
    }
    const box = $('sessions');
    const empty = box.querySelector('.sess-empty');
    if (list.length && empty) empty.remove();
    if (!list.length && !empty) box.append(h('p', { class: 'sess-empty' }, 'No sessions yet. Games you start will appear here.'));
    const rank = (x) => (x.state === 'running' ? 0 : x.state === 'queued' ? 1 : 2);
    const sorted = [...list].sort((a, b) => rank(a) - rank(b) || (rank(a) === 2 ? b.id - a.id : a.id - b.id));
    const seen = new Set();
    sorted.forEach((x) => {
      seen.add(String(x.id));
      let el = box.querySelector(`[data-id="${x.id}"]`);
      if (!el) { el = sessionEl(x); box.append(el); }
      paintSession(el, x);
    });
    box.querySelectorAll('.sess').forEach((el) => { if (!seen.has(el.dataset.id)) el.remove(); });
    const have = [...box.querySelectorAll('.sess')].map((e) => e.dataset.id).join();
    if (have !== sorted.map((x) => String(x.id)).join()) {
      const focused = document.activeElement;
      sorted.forEach((x) => box.append(box.querySelector(`[data-id="${x.id}"]`)));
      if (focused && focused.isConnected && focused !== document.activeElement) focused.focus();
    }
    const live = list.filter((x) => x.state === 'running').length;
    const waiting = list.filter((x) => x.state === 'queued').length;
    if (S.pending !== live + waiting) { S.pending = live + waiting; refreshStartLabels(); }
    $('dock-sub').textContent = list.length ? `${live} active · ${waiting} waiting` : '';
    $('clear-finished').hidden = !list.some((x) => !['running', 'queued'].includes(x.state) && !x.files_present);
  } catch {
    if (++pollFails === 3) toast('Lost contact with the app. Restart it if the list stays frozen.', 'err');
  } finally {
    polling = false;
    if (pollAgain) { pollAgain = false; poll(); }
  }
}

function requestClose() {
  if (S.pending > 0) confirmClose();
  else appWindow().close();
}

function confirmClose() {
  const old = $('close-dialog');
  if (old && old.open) return;
  if (old) old.remove();
  const running = S.list.find((x) => x.state === 'running');
  const waiting = S.list.filter((x) => x.state === 'queued').length;
  const line = running
    ? `${running.name} is running${waiting ? ` and ${waiting} more ${waiting === 1 ? 'game is' : 'games are'} waiting.` : '.'}`
    : `${waiting} ${waiting === 1 ? 'game is' : 'games are'} waiting.`;
  const hint = running
    ? `The running game stops${running.auto_delete ? ' and its files are deleted' : '. Its files stay in your folder'}. Waiting games are dropped.`
    : 'Waiting games are dropped. Nothing has been created for them yet.';
  const dlg = h('dialog', { id: 'close-dialog', 'aria-labelledby': 'cd-title' },
    h('h2', { id: 'cd-title' }, 'Close orbshacker?'), h('p', null, line), h('p', { class: 'hint' }, hint),
    h('div', { class: 'actions' },
      h('button', { type: 'button', class: 'btn', autofocus: true, onclick: () => { dlg.close(); dlg.remove(); } }, 'Keep open'),
      h('button', { type: 'button', class: 'btn quiet danger', onclick: () => call('quit_app').catch((e) => toast(e, 'err')) }, 'Stop and close'),
    ));
  dlg.addEventListener('close', () => dlg.remove());
  document.body.append(dlg);
  dlg.showModal();
}

document.addEventListener('keydown', (e) => {
  if (!e.ctrlKey || e.altKey || e.metaKey || e.shiftKey) return;
  const view = VIEW_ORDER[Number(e.key) - 1];
  if (view && S.boot) { e.preventDefault(); go(view); }
});

(async function boot() {
  $('win-min').addEventListener('click', () => appWindow().minimize());
  $('win-close').addEventListener('click', requestClose);
  $('clear-finished').addEventListener('click', async () => {
    const finished = S.list.filter((i) => !['running', 'queued'].includes(i.state));
    const clearable = finished.filter((i) => !i.files_present);
    for (const x of clearable) await call('session_dismiss', { id: x.id }).catch(() => {});
    const kept = finished.length - clearable.length;
    if (kept) toast(`${kept} ${kept === 1 ? 'game still has' : 'games still have'} files on disk. Delete those files first, then remove the card.`);
    await refreshSessions();
    if (document.activeElement === document.body) $('dock-title').focus();
  });
  try { window.__TAURI__.event.listen('close-requested', confirmClose); } catch {                                 }
  try {
    S.boot = await call('bootstrap');
  } catch (e) {
    $('view').replaceChildren(errBox('orbshacker couldn’t start. Close it and open it again. If this keeps happening, check that settings.json next to the exe is valid JSON.', e));
    return;
  }
  buildNav();
  go('discord');
  loadDb(false);
  poll();
  setInterval(() => { if (!document.hidden) poll(); }, 1000);
  document.addEventListener('visibilitychange', () => { if (!document.hidden) poll(); });
})();
