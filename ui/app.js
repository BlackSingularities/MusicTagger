import { icon, logo } from './icons.js';
import { t, setLang, currentLang } from './i18n.js';

// ═════════════════════════════ Most do backendu ═════════════════════════════
const TAURI = window.__TAURI__;
const invoke = (cmd, args) => (TAURI ? TAURI.core.invoke(cmd, args) : window.__MOCK__.invoke(cmd, args));
const listen = (ev, cb) => (TAURI ? TAURI.event.listen(ev, cb) : window.__MOCK__?.listen?.(ev, cb));
const fileSrc = (p) => (TAURI ? TAURI.core.convertFileSrc(p) : '');
const dialogApi = () => TAURI?.dialog;
const opener = () => TAURI?.opener;

// ═════════════════════════════ Stan ═════════════════════════════
const S = {
  settings: {},
  info: { version: '', platform: '' },
  tracks: [],
  byId: new Map(),
  proposals: new Map(),
  folder: null,
  view: 'library',
  quick: 'all',
  adv: { formats: new Set(), folders: new Set(), statuses: new Set(), missing: new Set(), min: 0, max: 100 },
  album: null,
  query: '',
  sort: { key: 'path', dir: 1 },
  mode: store('mode') || 'list',
  selected: new Set(),
  anchor: null,
  active: null,
  tab: 'tags',
  scan: null,
  jobs: { analyze: null, apply: null },
  covers: new Map(),
  coverSizes: new Map(),
  dupGroups: [],
  dupIndex: new Map(),
  visible: [],
  history: null,
  storageOk: true,
  rv: { conf: 'all', types: new Set(), q: '', active: null, sel: new Set(), detail: false, skip: new Set(JSON.parse(store('skipFields') || '[]')) },
};
const isMobile = () => matchMedia('(max-width: 760px)').matches;
const isAndroid = () => S.info.platform === 'android';

function store(k, v) {
  try {
    if (v === undefined) return localStorage.getItem('mt.' + k);
    localStorage.setItem('mt.' + k, v);
  } catch (_) { return null; }
}

// ═════════════════════════════ Pomocnicze ═════════════════════════════
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const fmtDur = (s) => {
  if (!s || s < 0) return '0:00';
  s = Math.round(s);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), x = s % 60;
  return (h ? h + ':' + String(m).padStart(2, '0') : m) + ':' + String(x).padStart(2, '0');
};
const fmtSize = (b) => (b > 1 << 30 ? (b / (1 << 30)).toFixed(2) + ' GB' : b > 1 << 20 ? (b / (1 << 20)).toFixed(1) + ' MB' : Math.max(1, Math.round(b / 1024)) + ' KB');
const scoreColor = (s) => (s >= 85 ? 'var(--success)' : s >= 60 ? 'var(--warn)' : 'var(--error)');
const confCls = (c) => (c >= (S.settings.autoApplyThreshold ?? 0.9) ? 'hi' : c >= 0.7 ? 'mid' : 'lo');
const debounce = (fn, ms) => { let h; return (...a) => { clearTimeout(h); h = setTimeout(() => fn(...a), ms); }; };
const errMsg = (e) => (typeof e === 'string' ? e : e?.message || JSON.stringify(e));
const baseName = (p) => String(p || '').split(/[\\/]/).filter(Boolean).pop() || p;
const fold = (s) => String(s || '').toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/ł/g, 'l');
const fieldLabel = (f) => t('f_' + f);
const CORE = ['title', 'artist', 'album', 'year', 'track'];
const MISS_KEYS = ['title', 'artist', 'album', 'albumArtist', 'year', 'track', 'genre', 'cover', 'coverSmall', 'lyrics', 'syncedLyrics'];
const STATUSES = ['idle', 'proposed', 'applied', 'upToDate', 'notFound', 'error'];

let rq = null;
const parts = new Set();
function schedule(what = 'all') {
  parts.add(what);
  if (rq) return;
  rq = requestAnimationFrame(() => {
    rq = null;
    const p = new Set(parts);
    parts.clear();
    if (p.has('all')) return renderAll();
    if (p.has('view')) renderView();
    else if (p.has('rows')) refreshLibrary();
    if (p.has('review')) refreshReview();
    if (p.has('sheet')) renderSheet();
    renderTopbar();
    renderNav();
    renderJobs();
  });
}

// ═════════════════════════════ Warstwy i przycisk „wstecz” ═════════════════════════════
// Każdy element zamykany przyciskiem wstecz (dialog, menu, panel) jest warstwą.
// Zamknięcie programowe cofa historię bez wywoływania obsługi (licznik suppress),
// dzięki czemu zamknięcie panelu nie czyści np. zaznaczenia.
const layers = [];
let suppressPop = 0;
function pushLayer(close) {
  layers.push(close);
  history.pushState({ l: layers.length }, '');
}
function dropLayer(close) {
  const i = layers.lastIndexOf(close);
  if (i < 0) return;
  layers.splice(i, 1);
  suppressPop++;
  history.back();
}
window.addEventListener('popstate', () => {
  if (suppressPop > 0) { suppressPop--; return; }
  const close = layers.pop();
  if (close) close(true);
});
function closeTop() {
  const c = layers[layers.length - 1];
  if (c) { c(false); return true; }
  return false;
}

// ═════════════════════════════ Snackbar, dialogi, menu ═════════════════════════════
function snack(msg, kind = 'info', action) {
  const box = $('#snack');
  while (box.children.length >= (isMobile() ? 1 : 2)) box.firstElementChild.remove();
  const el = document.createElement('div');
  el.className = 'snackbar ' + (kind === 'bad' ? 'bad' : '');
  el.innerHTML = `<span class="msg">${esc(msg)}</span>`;
  if (action) {
    const b = document.createElement('button');
    b.className = 'btn text';
    b.textContent = action.label;
    b.onclick = () => { close(); action.run(); };
    el.appendChild(b);
  }
  const x = document.createElement('button');
  x.className = 'ibtn';
  x.innerHTML = icon('x', 'sm');
  x.onclick = () => close();
  el.appendChild(x);
  const close = () => { el.classList.add('out'); setTimeout(() => el.remove(), 220); };
  box.appendChild(el);
  setTimeout(close, kind === 'bad' ? 9000 : action ? 8000 : 4000);
}

function dialog({ title, body, buttons = [], wide = false, onMount }) {
  return new Promise((resolve) => {
    const ov = document.createElement('div');
    ov.className = 'overlay';
    ov.innerHTML = `<div class="dialog ${wide ? 'wide' : ''}" role="dialog" aria-modal="true">
      <div class="dialog-head"><h3>${esc(title)}</h3></div>
      <div class="dialog-body">${body}</div>
      ${buttons.length ? `<div class="dialog-foot">${buttons.map((b, i) => `<button class="btn ${b.cls || 'text'} ${b.icon ? 'has-ic' : ''}" data-b="${i}">${b.icon ? icon(b.icon) : ''}${esc(b.label)}</button>`).join('')}</div>` : ''}</div>`;
    let done = false;
    const finish = (v, fromBack) => {
      if (done) return;
      done = true;
      ov.remove();
      if (!fromBack) dropLayer(close);
      resolve(v);
    };
    const close = (fromBack) => finish(null, fromBack);
    pushLayer(close);
    ov.addEventListener('mousedown', (e) => { if (e.target === ov) finish(null); });
    $$('[data-b]', ov).forEach((btn) => {
      btn.onclick = async () => {
        const b = buttons[+btn.dataset.b];
        if (!b.run) return finish(b.value ?? null);
        btn.disabled = true;
        try {
          const r = await b.run(ov);
          if (r !== false) finish(r ?? b.value ?? true);
        } catch (e) { snack(errMsg(e), 'bad'); }
        btn.disabled = false;
      };
    });
    $('#layers').appendChild(ov);
    onMount?.(ov, (v) => finish(v));
    setTimeout(() => $('input.inp, textarea', ov)?.focus(), 40);
  });
}
const confirmBox = (title, text, ok = t('ok'), danger = false) =>
  dialog({ title, body: `<p style="margin:0;line-height:1.5">${esc(text)}</p>`, buttons: [{ label: t('cancel'), value: false }, { label: ok, cls: danger ? 'filled danger' : 'filled', value: true }] });
const promptBox = (title, label, value = '', ph = '') =>
  dialog({
    title,
    body: `<div class="tf"><label>${esc(label)}</label><input class="inp" id="pb" value="${esc(value)}" placeholder="${esc(ph)}"></div>`,
    buttons: [{ label: t('cancel'), value: null }, { label: t('ok'), cls: 'filled', run: (ov) => $('#pb', ov).value }],
    onMount: (ov, finish) => { $('#pb', ov).onkeydown = (e) => { if (e.key === 'Enter') finish($('#pb', ov).value); }; },
  });

/** Menu przy elemencie. items: {icon,label,end,run,disabled,switch} | 'sep' | {header} */
function menu(anchor, items) {
  const m = document.createElement('div');
  m.className = 'menu';
  m.innerHTML = items.map((it, i) => {
    if (it === 'sep') return '<div class="divider"></div>';
    if (it.header) return `<div class="hdr">${esc(it.header)}</div>`;
    return `<div class="mi ${it.disabled ? 'disabled' : ''}" data-i="${i}">${it.icon ? icon(it.icon) : '<span style="width:20px"></span>'}<span class="ellipsis">${esc(it.label)}</span>${it.switch !== undefined ? `<span class="end"><button class="switch ${it.switch ? 'on' : ''}" tabindex="-1"></button></span>` : it.end ? `<span class="end">${esc(it.end)}</span>` : ''}</div>`;
  }).join('');
  let closed = false;
  const ov = document.createElement('div');
  const close = (fromBack) => {
    if (closed) return;
    closed = true;
    m.remove();
    ov.remove();
    if (!fromBack) dropLayer(close);
  };
  ov.style.cssText = 'position:fixed;inset:0;z-index:69';
  ov.onmousedown = () => close();
  document.body.append(ov, m);
  const r = anchor.getBoundingClientRect();
  const mw = Math.max(240, m.offsetWidth), mh = m.offsetHeight;
  let left = Math.min(r.right - mw, innerWidth - mw - 8);
  if (left < 8) left = Math.min(r.left, innerWidth - mw - 8);
  let top = r.bottom + 4;
  if (top + mh > innerHeight - 8) top = Math.max(8, r.top - mh - 4);
  m.style.left = Math.max(8, left) + 'px';
  m.style.top = top + 'px';
  pushLayer(close);
  m.onclick = (e) => {
    const el = e.target.closest('[data-i]');
    if (!el) return;
    const it = items[+el.dataset.i];
    if (it.switch !== undefined) {
      it.switch = !it.switch;
      $('.switch', el).classList.toggle('on', it.switch);
      it.run?.(it.switch);
      return;
    }
    close();
    it.run?.();
  };
}

// ═════════════════════════════ Dane ═════════════════════════════
function setTracks(list, keepState = false) {
  S.tracks = list;
  S.byId = new Map(list.map((x) => [x.id, x]));
  if (!keepState) {
    S.proposals.clear();
    S.selected.clear();
    S.covers.clear();
    S.album = null;
    S.rv.active = null;
    S.rv.sel.clear();
  }
}
function upsert(tr, proposal) {
  const old = S.byId.get(tr.id);
  if (old) {
    if (JSON.stringify(old.cover) !== JSON.stringify(tr.cover)) for (const k of [...S.covers.keys()]) if (k.startsWith(tr.id + ':')) S.covers.delete(k);
    Object.assign(old, tr);
  } else {
    S.tracks.push(tr);
    S.byId.set(tr.id, tr);
  }
  if (proposal) {
    applySkipFields(proposal);
    S.proposals.set(tr.id, proposal);
  } else if (!['proposed', 'error', 'queued', 'working'].includes(tr.status)) S.proposals.delete(tr.id);
}
function replaceTrack(oldId, tr) {
  const i = S.tracks.findIndex((x) => x.id === oldId);
  if (i >= 0) S.tracks[i] = tr; else S.tracks.push(tr);
  S.byId.delete(oldId);
  S.byId.set(tr.id, tr);
  const p = S.proposals.get(oldId);
  S.proposals.delete(oldId);
  if (p) { p.trackId = tr.id; S.proposals.set(tr.id, p); }
  if (S.selected.delete(oldId)) S.selected.add(tr.id);
  if (S.active === oldId) S.active = tr.id;
  if (S.rv.active === oldId) S.rv.active = tr.id;
}
/** Pola, których użytkownik nie chce zmieniać (globalnie, np. gatunek). */
function applySkipFields(p) {
  if (!S.rv.skip.size) return false;
  let ch = false;
  for (const c of p.changes) if (S.rv.skip.has(c.field) && c.enabled) { c.enabled = false; ch = true; }
  if (S.rv.skip.has('cover') && p.coverEnabled) { p.coverEnabled = false; ch = true; }
  if (S.rv.skip.has('lyrics') && p.lyricsEnabled) { p.lyricsEnabled = false; ch = true; }
  if (ch) invoke('update_proposal', { proposal: p });
  return ch;
}
const pendingProposals = () => S.tracks.filter((x) => x.status === 'proposed' && S.proposals.has(x.id));

// ═════════════════════════════ Filtry biblioteki ═════════════════════════════
const tagsComplete = (x) => !CORE.some((f) => x.missing.includes(f)) && !x.missing.includes('genre');
const QUICK = {
  all: () => true,
  missingTags: (x) => !tagsComplete(x),
  noCover: (x) => x.missing.includes('cover') || x.missing.includes('coverSmall'),
  noLyrics: (x) => x.missing.includes('lyrics'),
  noSynced: (x) => !x.lyricsSynced,
  proposed: (x) => x.status === 'proposed' && S.proposals.has(x.id),
  dups: (x) => S.dupIndex.has(x.id),
  problems: (x) => x.status === 'notFound' || x.status === 'error',
  complete: (x) => x.score >= 100,
};
const advCount = () => S.adv.formats.size + S.adv.folders.size + S.adv.statuses.size + S.adv.missing.size + (S.adv.min > 0 || S.adv.max < 100 ? 1 : 0);
function advMatch(x) {
  const a = S.adv;
  if (a.formats.size && !a.formats.has(x.format)) return false;
  if (a.folders.size && !a.folders.has(x.folder)) return false;
  if (a.statuses.size && !a.statuses.has(x.status === 'queued' || x.status === 'working' ? 'idle' : x.status)) return false;
  if (a.missing.size && ![...a.missing].some((m) => x.missing.includes(m))) return false;
  if (x.score < a.min || x.score > a.max) return false;
  return true;
}
const albumKey = (x) => (x.tags.album ? (x.tags.albumArtist || x.tags.artist || '') + '\u0001' + x.tags.album : '\u0002' + x.folder);
function matchesQuery(x, words) {
  if (!words.length) return true;
  const hay = fold([x.tags.title, x.tags.artist, x.tags.album, x.tags.albumArtist, x.tags.genre, x.tags.year, x.fileName].join(' '));
  return words.every((w) => hay.includes(w));
}
function computeVisible() {
  const words = fold(S.query).split(/\s+/).filter(Boolean);
  const q = QUICK[S.quick] || QUICK.all;
  let list = S.tracks.filter((x) => q(x) && advMatch(x) && matchesQuery(x, words) && (!S.album || albumKey(x) === S.album));
  const { key, dir } = S.sort;
  if (S.quick === 'dups') {
    list = list.slice().sort((a, b) => S.dupIndex.get(a.id) - S.dupIndex.get(b.id));
  } else if (key !== 'path') {
    const get = {
      title: (x) => fold(x.tags.title || x.fileName),
      artist: (x) => fold(x.tags.artist || '~'),
      album: (x) => fold(x.tags.album || '~') + String(x.tags.disc || 1).padStart(2, '0') + String(x.tags.track || 999).padStart(3, '0'),
      score: (x) => x.score,
      modified: (x) => x.modified,
      duration: (x) => x.duration,
    }[key];
    list = list.slice().sort((a, b) => { const A = get(a), B = get(b); return (A < B ? -1 : A > B ? 1 : 0) * dir; });
  } else if (dir < 0) list = list.slice().reverse();
  S.visible = list;
  return list;
}
function counts() {
  const c = {};
  for (const k of Object.keys(QUICK)) c[k] = 0;
  for (const x of S.tracks) for (const k of Object.keys(QUICK)) if (QUICK[k](x)) c[k]++;
  return c;
}

// ═════════════════════════════ Okładki ═════════════════════════════
const coverQueue = [];
let coverBusy = 0;
function wantCover(id, size) {
  const key = id + ':' + size;
  if (S.covers.has(key)) return S.covers.get(key);
  S.covers.set(key, undefined);
  coverQueue.push([id, size]);
  pumpCovers();
}
function pumpCovers() {
  while (coverBusy < 4 && coverQueue.length) {
    const [id, size] = coverQueue.pop();
    coverBusy++;
    invoke('get_cover', { id, size })
      .then((url) => {
        S.covers.set(id + ':' + size, url || null);
        if (url) $$(`[data-cover="${id}:${size}"]`).forEach((el) => { el.innerHTML = `<img src="${url}" alt="">`; });
      })
      .catch(() => S.covers.set(id + ':' + size, null))
      .finally(() => { coverBusy--; pumpCovers(); });
  }
}
function coverHTML(x, size = 96, cls = 'cover') {
  if (!x?.cover) return `<div class="${cls}">${icon('music')}</div>`;
  const url = wantCover(x.id, size);
  return `<div class="${cls}" data-cover="${x.id}:${size}">${url ? `<img src="${url}" alt="">` : icon('disc')}</div>`;
}
// Rzeczywista rozdzielczość okładek proponowanych przez źródła.
const probeQueue = [];
let probeBusy = 0;
function coverRes(url) {
  const v = S.coverSizes.get(url);
  if (v === undefined) { S.coverSizes.set(url, 'loading'); probeQueue.push(url); pumpProbe(); return 'loading'; }
  return v;
}
function pumpProbe() {
  while (probeBusy < 2 && probeQueue.length) {
    const url = probeQueue.shift();
    probeBusy++;
    invoke('probe_cover', { url })
      .then((r) => S.coverSizes.set(url, r))
      .catch(() => S.coverSizes.set(url, 'err'))
      .finally(() => {
        probeBusy--;
        $$('[data-res]').forEach((el) => { if (el.dataset.res === url) el.outerHTML = resHTML(url); });
        pumpProbe();
      });
  }
}
function resHTML(url) {
  const r = S.coverSizes.get(url);
  const min = S.settings.minCoverSize || 500;
  if (!r || r === 'loading') return `<div class="res" data-res="${esc(url)}">${esc(t('checking'))}</div>`;
  if (r === 'err') return `<div class="res low" data-res="${esc(url)}">${esc(t('unavailable'))}</div>`;
  return `<div class="res ${r.width < min ? 'low' : r.width >= 1000 ? 'good' : ''}" data-res="${esc(url)}">${r.width}×${r.height} px · ${fmtSize(r.bytes)}</div>`;
}

// ═════════════════════════════ Nawigacja ═════════════════════════════
const NAV = [
  { id: 'library', icon: 'music', label: 'nav_library' },
  { id: 'review', icon: 'sparkles', label: 'nav_review' },
  { id: 'history', icon: 'history', label: 'nav_history' },
  { id: 'settings', icon: 'settings', label: 'nav_settings' },
];
function go(view, opts = {}) {
  if (S.view === view && !opts.force) return;
  S.view = view;
  if (view === 'history') loadHistory();
  if (view === 'review') {
    S.rv.detail = false;
    if (opts.active) S.rv.active = opts.active;
  }
  renderAll();
  if (view === 'review' && opts.active && isMobile()) openReviewDetail();
}
function navBadge(id) {
  if (id === 'review') { const n = pendingProposals().length; return n ? `<span class="badge">${n > 999 ? '999+' : n}</span>` : ''; }
  return '';
}
function renderNav() {
  const html = NAV.map((n) => `<button class="rail-item ${S.view === n.id ? 'active' : ''}" data-nav="${n.id}"><span class="pill">${icon(n.icon, 'lg')}${navBadge(n.id)}</span><span class="lbl">${esc(t(n.label))}</span></button>`).join('');
  $('#rail').innerHTML = `<div class="logo">${logo()}</div>${html}<div class="rail-spacer"></div>
    <button class="ibtn" data-theme-toggle title="${esc(t('theme'))}">${icon(S.settings.theme === 'light' ? 'moon' : 'sun')}</button>`;
  $('#navbar').innerHTML = html;
}

// ═════════════════════════════ Pasek górny ═════════════════════════════
function renderTopbar() {
  const tb = $('#topbar');
  tb.classList.remove('ctx');
  if (S.view === 'library' && S.selected.size) {
    const ids = [...S.selected];
    const withProp = ids.filter((id) => S.proposals.has(id) && S.byId.get(id)?.status === 'proposed').length;
    tb.classList.add('ctx');
    tb.innerHTML = `<button class="ibtn" data-act="sel-clear" title="${esc(t('clearSel'))}">${icon('x')}</button>
      <div class="title grow"><h1>${esc(t('selected', { n: S.selected.size }))}</h1></div>
      <button class="btn text has-ic hide-m" data-act="sel-analyze">${icon('sparkles')}${esc(t('smartFixSel'))}</button>
      <button class="ibtn" data-act="sel-analyze" style="${isMobile() ? '' : 'display:none'}">${icon('sparkles')}</button>
      ${withProp ? `<button class="ibtn" data-act="sel-accept" title="${esc(t('acceptN', { n: withProp }))}">${icon('checkAll')}</button>` : ''}
      <button class="ibtn" data-act="sel-edit" title="${esc(t('batchEdit'))}">${icon('edit')}</button>
      <button class="ibtn" data-act="sel-rename" title="${esc(t('rename'))}">${icon('rename')}</button>
      <button class="ibtn" data-act="sel-more" title="${esc(t('more'))}">${icon('more')}</button>`;
    return;
  }
  const title = { library: t('nav_library'), review: t('nav_review'), history: t('nav_history'), settings: t('nav_settings') }[S.view];
  let sub = '', actions = '';
  if (S.view === 'library') {
    sub = S.folder ? `${S.folder}${S.tracks.length ? ' · ' + t('nTracks', { n: S.tracks.length }) : ''}` : t('libSub');
    const search = S.tracks.length
      ? isMobile() ? `<div class="grow"></div><button class="ibtn ${S.query ? 'on' : ''}" data-act="search-m">${icon('search')}</button>` : `<div class="search">${icon('search')}<input id="q" placeholder="${esc(t('searchPh'))}" value="${esc(S.query)}"></div>`
      : '<div class="grow"></div>';
    actions = `${search}<button class="btn tonal has-ic" data-act="open-menu" style="margin-left:8px">${icon('folderOpen')}<span class="hide-m">${esc(t('open'))}</span></button>
      ${S.tracks.length ? `<button class="ibtn" data-act="lib-more" title="${esc(t('more'))}">${icon('more')}</button>` : ''}`;
  } else if (S.view === 'review') {
    const list = pendingProposals();
    const th = S.settings.autoApplyThreshold ?? 0.9;
    const sure = list.filter((x) => S.proposals.get(x.id).confidence >= th).length;
    const queued = S.jobs.apply?.running ? S.jobs.apply.total - S.jobs.apply.done : 0;
    sub = (list.length ? t('reviewSub', { n: list.length, s: sure }) : t('reviewSubEmpty')) + (queued ? ' · ' + t('savingQueue', { n: queued }) : '');
    actions = `<div class="grow"></div>${list.length ? `<button class="btn filled has-ic" data-act="rv-accept-sure" ${sure ? '' : 'disabled'}>${icon('checkAll')}<span>${esc(isMobile() ? String(sure) : t('acceptSure', { n: sure }))}</span></button>
      <button class="ibtn" data-act="rv-more" title="${esc(t('more'))}">${icon('more')}</button>` : ''}`;
  } else if (S.view === 'history') {
    sub = t('historySub');
    actions = `<div class="grow"></div>${S.history?.length ? `<button class="btn text danger has-ic" data-act="h-clear">${icon('trash')}<span class="hide-m">${esc(t('clearHistory'))}</span></button>` : ''}`;
  } else {
    sub = t('settingsSub');
    actions = '<div class="grow"></div>';
  }
  tb.innerHTML = `<div class="title" style="max-width:${S.view === 'library' && S.tracks.length && !isMobile() ? '32%' : '70%'}"><h1>${esc(title)}</h1><div class="sub" title="${esc(sub)}">${esc(sub)}</div></div>${actions}`;
}

function renderJobs() {
  // Karta postępu leży w doku nad treścią (#dock), więc nie zmienia układu widoku.
  // Przy aktualizacji zmieniamy tylko tekst i szerokość paska, bez przebudowy DOM.
  const box = $('#jobs');
  const rows = [];
  const sc = S.scan;
  if (sc && sc.phase !== 'done') {
    rows.push({ key: 'scan', ic: icon('refresh', 'sm spin'), text: sc.total ? t('scanning', { done: sc.done, total: sc.total }) : t('collecting'), pct: sc.total ? (sc.done / sc.total) * 100 : null });
  }
  for (const kind of ['analyze', 'apply']) {
    const j = S.jobs[kind];
    if (!j?.running) continue;
    rows.push({ key: kind, ic: icon(kind === 'apply' ? 'download' : 'sparkles', 'sm'), text: t(kind === 'apply' ? 'applying' : 'analyzing', { done: j.done, total: j.total }), pct: j.total ? (j.done / j.total) * 100 : 0 });
  }
  document.body.classList.toggle('has-jobs', rows.length > 0);
  if (!rows.length) { box.innerHTML = ''; return; }
  const sig = rows.map((r) => r.key + (r.pct === null ? '?' : '')).join(',');
  let card = $('.jobs-card', box);
  if (!card || card.dataset.sig !== sig) {
    box.innerHTML = `<div class="jobs-card" data-sig="${sig}">${rows.map((r) => `<div class="job" data-job="${r.key}">${r.ic}<span class="txt"></span><span class="row" style="gap:0"><span class="pct"></span><button class="btn text sm" data-act="cancel">${esc(t('stop'))}</button></span><div class="lp ${r.pct === null ? 'indet' : ''}"><i></i></div></div>`).join('')}</div>`;
    card = $('.jobs-card', box);
  }
  for (const r of rows) {
    const el = $(`[data-job="${r.key}"]`, card);
    const txt = $('.txt', el);
    if (txt.textContent !== r.text) { txt.textContent = r.text; txt.title = r.text; }
    $('.pct', el).textContent = r.pct === null ? '' : Math.floor(r.pct) + '%';
    if (r.pct !== null) $('.lp > i', el).style.width = r.pct + '%';
  }
}

// ═════════════════════════════ Widoki ═════════════════════════════
function renderAll() {
  document.documentElement.lang = currentLang();
  renderNav();
  renderTopbar();
  renderJobs();
  renderView();
  renderSheet();
}
function renderView() {
  const v = $('#view');
  document.body.classList.remove('has-fab', 'has-actions');
  if (S.view === 'review') return renderReview(v);
  if (S.view === 'history') return renderHistory(v);
  if (S.view === 'settings') return renderSettings(v);
  return renderLibrary(v);
}

// ─────────────── Biblioteka ───────────────
function renderStart(v) {
  const recent = S.settings.recentFolders || [];
  v.innerHTML = `<div class="empty">
    ${!S.storageOk ? `<div class="banner err" style="max-width:560px;text-align:left">${icon('shield', 'lg')}<div class="grow"><b>${esc(t('permTitle'))}</b><div class="t-body-s" style="margin-top:4px">${esc(t('permText'))}</div></div><button class="btn text" data-act="perm-check">${esc(t('permCheck'))}</button></div>` : ''}
    <div class="hero-logo">${logo()}</div>
    <h2>${esc(t('welcomeTitle'))}</h2>
    <p>${esc(t('welcomeText'))}</p>
    <div class="card start-card">
      <div class="li" data-act="open-folder"><div class="ico">${icon('folderOpen')}</div><div class="grow"><div class="t-title-s">${esc(t('openFolder'))}</div><div class="t-body-s muted">${esc(isAndroid() ? t('openFolderHintA') : t('openFolderHint'))}</div></div>${icon('chevronRight', 'dim')}</div>
      ${!isAndroid() ? `<div class="li" data-act="open-files"><div class="ico">${icon('file')}</div><div class="grow"><div class="t-title-s">${esc(t('openFiles'))}</div><div class="t-body-s muted">${esc(t('openFilesHint'))}</div></div>${icon('chevronRight', 'dim')}</div>` : ''}
      <div class="li" data-act="toggle-recursive"><div class="ico" style="background:var(--surface-highest);color:var(--on-surface-var)">${icon('layers')}</div><div class="grow"><div class="t-title-s">${esc(t('recursive'))}</div><div class="t-body-s muted">${esc(t('recursiveHint'))}</div></div><button class="switch ${S.settings.recursive !== false ? 'on' : ''}" tabindex="-1"></button></div>
      ${recent.length ? `<div class="divider" style="margin:8px 0"></div><div class="t-label muted" style="padding:4px 16px">${esc(t('recent'))}</div>
        ${recent.slice(0, 5).map((f) => `<div class="li" data-open="${esc(f)}"><div class="ico" style="background:var(--surface-highest);color:var(--on-surface-var)">${icon('history')}</div><div class="grow"><div class="t-title-s ellipsis">${esc(baseName(f))}</div><div class="t-body-s muted ellipsis">${esc(f)}</div></div></div>`).join('')}` : ''}
    </div>
    ${isAndroid() ? '' : `<p class="t-body-s">${esc(t('orDrop'))}</p>`}
    <div class="feature-grid">
      <div class="card feature">${icon('tag', 'lg')}<b>${esc(t('feat1'))}</b><span>${esc(t('feat1d'))}</span></div>
      <div class="card feature">${icon('image', 'lg')}<b>${esc(t('feat2'))}</b><span>${esc(t('feat2d'))}</span></div>
      <div class="card feature">${icon('lyrics', 'lg')}<b>${esc(t('feat3'))}</b><span>${esc(t('feat3d'))}</span></div>
    </div></div>`;
}
function renderLibrary(v) {
  if (!S.tracks.length) {
    if (S.scan && S.scan.phase !== 'done') { v.innerHTML = `<div class="empty">${icon('refresh', 'xl spin')}<h2>${esc(t('scanningTitle'))}</h2><p>${esc(S.folder || '')}</p></div>`; return; }
    if (S.folder) {
      v.innerHTML = `<div class="empty">${icon('music', 'xl')}<h2>${esc(t('noAudio'))}</h2><p>${esc(t(S.settings.recursive === false ? 'noAudioTextFlat' : 'noAudioText'))}</p>
        <div class="row wrap" style="justify-content:center"><button class="btn filled has-ic" data-act="open-folder">${icon('folderOpen')}${esc(t('openFolder'))}</button>
        ${S.settings.recursive === false ? `<button class="btn tonal" data-act="rescan-recursive">${esc(t('rescanWithSub'))}</button>` : ''}</div></div>`;
      return;
    }
    return renderStart(v);
  }
  v.innerHTML = `<div class="lib-head"><div id="summary"></div><div class="lib-tools" id="tools"></div></div><div id="listwrap" style="flex:1;min-height:0;display:flex;flex-direction:column"></div>
    <button class="fab ${S.selected.size ? 'hide' : ''}" data-act="fab">${icon('sparkles')}<span class="lbl">${esc(t('smartFix'))}</span></button>`;
  document.body.classList.add('has-fab');
  refreshLibrary(true);
}
function refreshLibrary(full) {
  if (S.view !== 'library') return;
  if (!S.tracks.length || !$('#summary')) return renderView();
  computeVisible();
  renderSummary();
  renderTools();
  const w = $('#listwrap');
  if (S.mode === 'albums' && !S.album) renderAlbums(w);
  else if (full || !$('#lscroll')) renderList(w);
  else paintRows();
  $('.fab')?.classList.toggle('hide', S.selected.size > 0);
}
function renderSummary() {
  const n = S.tracks.length;
  const avg = Math.round(S.tracks.reduce((a, x) => a + x.score, 0) / n);
  const okTags = S.tracks.filter(tagsComplete).length;
  const okCover = S.tracks.filter((x) => !QUICK.noCover(x)).length;
  const okLyr = S.tracks.filter((x) => x.lyricsSynced).length;
  const need = S.tracks.filter((x) => x.score < 100).length;
  const bar = (k, label, ok) => `<div class="sum-bar" data-quick="${k}" title="${esc(t('showMissing'))}"><div class="row"><span>${esc(label)}</span><b>${ok}/${n}</b></div><div class="lp"><i style="width:${(ok / n) * 100}%"></i></div></div>`;
  $('#summary').innerHTML = `<div class="summary">
    <svg class="ring" viewBox="0 0 64 64"><circle class="bg" cx="32" cy="32" r="27"/><circle class="fg" cx="32" cy="32" r="27" stroke-dasharray="169.6" style="stroke-dashoffset:${169.6 * (1 - avg / 100)}"/><text x="32" y="33">${avg}%</text></svg>
    <div class="sum-main"><div class="big">${esc(t('nTracks', { n }))}</div><div class="t-body-s muted">${need ? esc(t('needWork', { n: need })) : esc(t('allComplete'))}</div></div>
    <div class="sum-bars">${bar('missingTags', t('sumTags'), okTags)}${bar('noCover', t('sumCovers'), okCover)}${bar('noSynced', t('sumLyrics'), okLyr)}</div></div>`;
}
function renderTools() {
  const c = counts();
  // Chipy są zawsze widoczne (puste wyszarzone), żeby rząd nie przeskakiwał, gdy liczniki rosną.
  const chip = (k, ic) => (`<button class="chip ${S.quick === k ? 'on' : ''} ${k !== 'all' && !c[k] && S.quick !== k ? 'zero' : ''}" data-quick="${k}">${icon('check', 'chk')}${ic && S.quick !== k ? icon(ic) : ''}${esc(t('q_' + k))}<span class="n">${c[k]}</span></button>`);
  const advN = advCount();
  const advChips = [];
  for (const f of S.adv.formats) advChips.push(['formats', f, f]);
  for (const f of S.adv.folders) advChips.push(['folders', f, baseName(f)]);
  for (const f of S.adv.statuses) advChips.push(['statuses', f, t('st_' + f)]);
  for (const f of S.adv.missing) advChips.push(['missing', f, t('missPrefix') + ' ' + t('mi_' + f)]);
  if (S.adv.min > 0 || S.adv.max < 100) advChips.push(['range', '', `${S.adv.min}–${S.adv.max}%`]);
  const albumName = S.album ? (S.album.startsWith('\u0002') ? baseName(S.album.slice(1)) : S.album.split('\u0001')[1]) : '';
  const qchip = S.query && isMobile() ? `<button class="chip input" data-rm="query">${icon('search', 'sm')}${esc(S.query)}<span class="x">${icon('x', 'sm')}</span></button>` : '';
  $('#tools').innerHTML = `<div class="chips">
      <button class="chip ${advN ? 'on' : ''}" data-act="adv-open">${icon('tune')}${esc(t('filters'))}${advN ? `<span class="n">${advN}</span>` : ''}</button>
      ${qchip}
      ${S.album ? `<button class="chip input" data-rm="album">${icon('disc', 'sm')}${esc(albumName)}<span class="x">${icon('x', 'sm')}</span></button>` : ''}
      ${advChips.map(([k, v, l]) => `<button class="chip input" data-rm="${k}" data-v="${esc(v)}">${esc(l)}<span class="x">${icon('x', 'sm')}</span></button>`).join('')}
      ${chip('all')}${chip('missingTags')}${chip('noCover')}${chip('noLyrics')}${chip('noSynced')}${chip('proposed', 'sparkles')}${chip('dups', 'duplicate')}${chip('problems', 'alert')}${chip('complete')}
    </div>
    <button class="ibtn" data-act="sort-menu" title="${esc(t('sortBy'))}">${icon('sort')}</button>
    <div class="seg sm"><button class="${S.mode === 'list' ? 'on' : ''}" data-mode="list" title="${esc(t('viewList'))}">${icon('list', 'sm')}</button><button class="${S.mode === 'albums' ? 'on' : ''}" data-mode="albums" title="${esc(t('viewAlbums'))}">${icon('grid', 'sm')}</button></div>`;
}
const rowH = () => parseInt(getComputedStyle(document.documentElement).getPropertyValue('--row-h')) || 64;
function renderList(el) {
  const sh = (k, label) => `<span class="sortable" data-sort="${k}">${esc(label)}${S.sort.key === k ? (S.sort.dir > 0 ? ' ↑' : ' ↓') : ''}</span>`;
  el.innerHTML = `<div class="list-head"><span class="cbw" data-act="sel-all" title="Ctrl+A"><span class="cb">${icon('check')}</span></span><span></span>
    ${sh('title', t('colTitle'))}<span class="c-album">${sh('album', t('colAlbum'))}</span><span class="c-miss">${esc(t('colMissing'))}</span><span class="c-score">${sh('score', t('colScore'))}</span><span>${esc(t('colStatus'))}</span></div>
    <div class="list-scroll" id="lscroll"><div class="list-inner" id="linner"></div></div>`;
  if (!S.visible.length) {
    $('#lscroll').innerHTML = `<div class="empty">${icon('filter', 'xl')}<p>${esc(t('nothingHere'))}</p><button class="btn text" data-act="clear-filters">${esc(t('clearFilters'))}</button></div>`;
    return;
  }
  $('#lscroll').addEventListener('scroll', () => requestAnimationFrame(paintRows), { passive: true });
  bindRows($('#linner'));
  paintRows();
}
function missIcons(x) {
  const m = x.missing;
  const tg = tagsComplete(x) ? 'ok' : x.tags.title && x.tags.artist ? 'half' : 'no';
  const cv = m.includes('cover') ? 'no' : m.includes('coverSmall') ? 'half' : 'ok';
  const ly = m.includes('lyrics') ? 'no' : m.includes('syncedLyrics') ? 'half' : 'ok';
  const tip = (k, c) => esc(t('miss_' + k + '_' + c));
  return `<div class="miss"><span class="m ${tg}" title="${tip('tags', tg)}">${icon('tag')}</span><span class="m ${cv}" title="${tip('cover', cv)}">${icon('image')}</span><span class="m ${ly}" title="${tip('lyrics', ly)}">${icon('lyrics')}</span></div>`;
}
function statusHTML(x) {
  if (x.status === 'idle') return '<span class="st idle"></span>';
  const p = S.proposals.get(x.id);
  const ic = { queued: 'clock', working: 'refresh', proposed: 'sparkles', applied: 'check', upToDate: 'checkAll', notFound: 'help', error: 'alert' }[x.status];
  const label = x.status === 'proposed' && p ? Math.round(p.confidence * 100) + '%' : t('st_' + x.status);
  return `<span class="st ${x.status}" title="${esc(x.error || t('st_' + x.status))}">${ic ? icon(ic, x.status === 'working' ? 'spin' : '') : ''}<span class="lbl">${esc(label)}</span></span>`;
}
function rowHTML(x, top) {
  const sub = [x.tags.artist || t('unknownArtist'), x.tags.year].filter(Boolean).join(' · ');
  const sel = S.selected.has(x.id);
  return `<div class="trow ${sel ? 'sel' : ''} ${S.active === x.id ? 'active' : ''}" style="top:${top}px" data-id="${x.id}">
    <span class="cbw" data-cb><span class="cb ${sel ? 'on' : ''}">${icon('check')}</span></span>
    ${coverHTML(x)}
    <div class="grow"><div class="t1 ellipsis" title="${esc(x.path)}">${esc(x.tags.title || x.fileName)}</div><div class="t2 ellipsis">${esc(sub)}</div></div>
    <div class="c-album grow"><div class="ellipsis">${esc(x.tags.album || '—')}</div><div class="t2 ellipsis">${esc(x.format)} · ${fmtDur(x.duration)}${x.tags.track ? ' · #' + x.tags.track : ''}</div></div>
    <div class="c-miss">${missIcons(x)}</div>
    <div class="c-score score"><div class="bar"><i style="width:${x.score}%;background:${scoreColor(x.score)}"></i></div><b>${x.score}</b></div>
    <div class="c-status">${statusHTML(x)}</div></div>`;
}
function paintRows() {
  const sc = $('#lscroll'), inner = $('#linner');
  if (!sc || !inner) return;
  const h = rowH(), list = S.visible;
  inner.style.height = list.length * h + 120 + 'px';
  const start = Math.max(0, Math.floor(sc.scrollTop / h) - 6);
  const end = Math.min(list.length, Math.ceil((sc.scrollTop + sc.clientHeight) / h) + 6);
  let html = '';
  for (let i = start; i < end; i++) html += rowHTML(list[i], i * h);
  inner.innerHTML = html;
  const sa = $('[data-act="sel-all"] .cb');
  if (sa) {
    const all = list.length && list.every((x) => S.selected.has(x.id));
    sa.className = 'cb ' + (all ? 'on' : list.some((x) => S.selected.has(x.id)) ? 'part' : '');
  }
}
function bindRows(inner) {
  let timer = null, long = false;
  inner.addEventListener('pointerdown', (e) => {
    const row = e.target.closest('.trow');
    if (!row || e.pointerType === 'mouse') return;
    long = false;
    timer = setTimeout(() => { long = true; toggleSel(row.dataset.id); navigator.vibrate?.(15); }, 450);
  });
  const cancel = () => clearTimeout(timer);
  ['pointerup', 'pointermove', 'pointercancel'].forEach((ev) => inner.addEventListener(ev, cancel));
  inner.addEventListener('click', (e) => {
    const row = e.target.closest('.trow');
    if (!row) return;
    e.stopPropagation();
    const id = row.dataset.id;
    if (long) { long = false; return; }
    if (e.target.closest('[data-cb]') || e.ctrlKey || e.metaKey) return toggleSel(id, e.shiftKey);
    if (e.shiftKey) return toggleSel(id, true);
    if (S.selected.size && isMobile()) return toggleSel(id);
    openSheet(id);
  });
}
function toggleSel(id, range) {
  if (range && S.anchor) {
    const ids = S.visible.map((x) => x.id);
    const a = ids.indexOf(S.anchor), b = ids.indexOf(id);
    if (a >= 0 && b >= 0) for (let i = Math.min(a, b); i <= Math.max(a, b); i++) S.selected.add(ids[i]);
  } else {
    S.selected.has(id) ? S.selected.delete(id) : S.selected.add(id);
    S.anchor = id;
  }
  paintRows();
  renderTopbar();
  $('.fab')?.classList.toggle('hide', S.selected.size > 0);
}
function renderAlbums(el) {
  const groups = new Map();
  for (const x of S.visible) { const k = albumKey(x); if (!groups.has(k)) groups.set(k, []); groups.get(k).push(x); }
  const arr = [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  el.innerHTML = `<div class="albums">${arr.map(([k, list]) => {
    const wc = list.find((x) => x.cover) || list[0];
    const avg = Math.round(list.reduce((a, x) => a + x.score, 0) / list.length);
    return `<div class="album" data-album="${esc(k)}">${coverHTML(wc, 300)}
      <div class="t1 ellipsis">${esc(list[0].tags.album || baseName(list[0].folder))}</div><div class="t2 ellipsis">${esc(list[0].tags.albumArtist || list[0].tags.artist || t('unknownArtist'))}</div>
      <div class="meta"><span class="t-body-s muted">${esc(t('nTracks', { n: list.length }))}</span><span class="t-label" style="color:${scoreColor(avg)}">${avg}%</span></div></div>`;
  }).join('')}</div>`;
}

// ─────────────── Arkusz boczny: szczegóły utworu ───────────────
let sheetLayer = null;
function openSheet(id, tab) {
  const was = !!S.active;
  S.active = id;
  if (tab) S.tab = tab;
  if (!was) {
    sheetLayer = (fromBack) => closeSheet(fromBack);
    pushLayer(sheetLayer);
  }
  stopAudio();
  renderSheet();
  paintRows();
}
function closeSheet(fromBack) {
  if (!S.active) return;
  S.active = null;
  stopAudio();
  renderSheet();
  paintRows();
  if (!fromBack && sheetLayer) dropLayer(sheetLayer);
  sheetLayer = null;
}
/** Przejście do kolejnego utworu w bieżącej liście (np. po zapisie). */
function stepSheet(dir = 1) {
  const src = S.view === 'review' ? reviewList() : S.visible;
  const ids = src.map((x) => x.id);
  const i = ids.indexOf(S.active);
  const n = i < 0 ? null : ids[i + dir];
  if (!n) { snack(t(dir > 0 ? 'lastTrack' : 'firstTrack')); return false; }
  openSheet(n);
  const sc = $('#lscroll');
  if (sc) {
    const top = ids.indexOf(n) * rowH();
    if (top < sc.scrollTop || top > sc.scrollTop + sc.clientHeight - rowH()) sc.scrollTop = top - sc.clientHeight / 2;
  }
  return true;
}
function renderSheet() {
  const d = $('#sheet');
  const x = S.active && S.byId.get(S.active);
  document.body.classList.toggle('sheet-open', !!x);
  if (!x) { d.classList.remove('open'); d.setAttribute('aria-hidden', 'true'); d.innerHTML = ''; return; }
  d.classList.add('open');
  d.setAttribute('aria-hidden', 'false');
  const tabs = [['tags', 'tag', 'tabTags'], ['cover', 'image', 'tabCover'], ['lyrics', 'lyrics', 'tabLyrics'], ['file', 'file', 'tabFile']];
  if (!tabs.some(([k]) => k === S.tab)) S.tab = 'tags';
  d.innerHTML = `<div class="sheet-inner">
    <div class="sheet-head">${coverHTML(x, 300, 'cover lg')}
      <div class="grow"><h2>${esc(x.tags.title || x.fileName)}</h2><div class="muted ellipsis">${esc(x.tags.artist || t('unknownArtist'))}${x.tags.album ? ' — ' + esc(x.tags.album) : ''}</div>
        <div class="row" style="margin-top:8px">${statusHTML(x)}<div class="score grow" style="max-width:160px"><div class="bar"><i style="width:${x.score}%;background:${scoreColor(x.score)}"></i></div><b>${x.score}</b></div></div></div>
      <div class="col" style="gap:0"><button class="ibtn" data-act="sheet-close" title="Esc">${icon('x')}</button><button class="ibtn" data-act="sheet-prev" title="↑">${icon('chevronUp')}</button><button class="ibtn" data-act="sheet-next" title="↓">${icon('chevronDown')}</button></div>
    </div>
    <div class="tabs">${tabs.map(([k, ic, l]) => `<button class="tab ${S.tab === k ? 'on' : ''}" data-tab="${k}">${icon(ic, 'sm')}${esc(t(l))}</button>`).join('')}</div>
    <div class="sheet-body" id="sbody"></div><div class="sheet-foot" id="sfoot"></div></div>`;
  const body = $('#sbody'), foot = $('#sfoot');
  const p = S.proposals.get(x.id);
  let banner = '';
  if (p && x.status === 'proposed') banner = `<div class="banner">${icon('sparkles', 'lg')}<div class="grow"><b>${esc(t('hasProposal', { p: Math.round(p.confidence * 100) }))}</b><div class="t-body-s">${esc(proposalSummary(p))}</div></div><button class="btn text" data-act="sheet-compare">${esc(t('compare'))}</button></div>`;
  else if (x.status === 'notFound') banner = `<div class="banner err">${icon('help', 'lg')}<div class="grow t-body-s">${esc(x.error || t('notFoundHint'))}</div><button class="btn text" data-act="manual-search">${esc(t('manualSearch'))}</button></div>`;
  else if (x.error && x.status === 'error') banner = `<div class="banner err">${icon('alert', 'lg')}<div class="grow t-body-s selectable">${esc(x.error)}</div></div>`;
  ({ tags: tabTags, cover: tabCover, lyrics: tabLyrics, file: tabFile }[S.tab])(x, body, foot);
  if (banner) body.insertAdjacentHTML('afterbegin', banner);
}
function proposalSummary(p) {
  const out = [];
  const n = p.changes.filter((c) => c.enabled).length;
  if (n) out.push(t('nFields', { n }));
  if (p.coverEnabled) out.push(t('coverWord'));
  if (p.lyricsEnabled) out.push(p.lyrics?.synced ? t('syncedWord') : t('lyricsWord'));
  return out.join(' · ') || t('nothingToChange');
}

function tabTags(x, body, foot) {
  const v = (f) => x.tags[f] ?? '';
  const inp = (f, cls = '') => `<div class="tf ${cls}"><label>${esc(fieldLabel(f))}${!x.tags[f] && CORE.includes(f) ? `<span class="tag warn" style="height:18px">${esc(t('missing'))}</span>` : ''}</label><input class="inp ${!x.tags[f] ? 'blank' : ''}" data-f="${f}" value="${esc(v(f))}"></div>`;
  body.innerHTML = `<div class="form">${inp('title', 'full')}${inp('artist')}${inp('albumArtist')}${inp('album', 'full')}
    <div class="tf"><label>${esc(t('f_track'))}</label><div class="pair"><input class="inp" data-f="track" value="${esc(v('track'))}" inputmode="numeric"><span class="dim">/</span><input class="inp" data-f="trackTotal" value="${esc(v('trackTotal'))}" inputmode="numeric"></div></div>
    <div class="tf"><label>${esc(t('f_disc'))}</label><div class="pair"><input class="inp" data-f="disc" value="${esc(v('disc'))}" inputmode="numeric"><span class="dim">/</span><input class="inp" data-f="discTotal" value="${esc(v('discTotal'))}" inputmode="numeric"></div></div>
    ${inp('year')}${inp('genre')}${inp('composer')}${inp('label')}${inp('isrc')}${inp('bpm')}${inp('comment', 'full')}</div>
    <div class="sec-title">${icon('sparkles')}${esc(t('smart'))}</div>
    <div class="row wrap"><button class="btn tonal sm has-ic" data-act="analyze-one">${icon('sparkles')}${esc(t('smartFixOne'))}</button><button class="btn outlined sm has-ic" data-act="manual-search">${icon('search')}${esc(t('manualSearch'))}</button>
      <button class="btn text sm" id="e-fromname">${esc(t('fromFilename'))}</button><button class="btn text sm" id="e-case">${esc(t('fixCase'))}</button></div>`;
  foot.innerHTML = `<span class="t-body-s muted grow" id="e-dirty">${esc(t('saveHint'))}</span><button class="btn text" id="e-reset">${esc(t('reset'))}</button><button class="btn filled has-ic" id="e-save" disabled>${icon('check')}${esc(t('saveNext'))}</button>`;
  const changed = () => {
    const ch = {};
    $$('[data-f]', body).forEach((el) => {
      const f = el.dataset.f, nv = el.value.trim(), ov = String(x.tags[f] ?? '');
      el.classList.toggle('changed', nv !== ov);
      if (nv !== ov) ch[f] = nv || null;
    });
    const n = Object.keys(ch).length;
    $('#e-save').disabled = !n;
    $('#e-dirty').textContent = n ? t('unsaved', { n }) : t('saveHint');
    return ch;
  };
  body.addEventListener('input', changed);
  body.addEventListener('keydown', (e) => { if (e.key === 'Enter' && e.target.matches('.inp') && !$('#e-save').disabled) $('#e-save').click(); });
  $('#e-reset').onclick = () => renderSheet();
  $('#e-save').onclick = async () => {
    const fields = changed();
    $('#e-save').disabled = true;
    try {
      await invoke('save_edits', { req: { ids: [x.id], fields } });
      snack(t('savedTrack', { t: fields.title || x.tags.title || x.fileName }));
      stepSheet(1);
    } catch (e) { snack(errMsg(e), 'bad'); $('#e-save').disabled = false; }
  };
  $('#e-fromname').onclick = () => {
    const stem = x.fileName.replace(/\.[^.]+$/, '').replace(/_/g, ' ');
    const pts = stem.replace(/^\s*\d{1,3}\s*[-.)_ ]\s*/, '').split(/\s+[-–]\s+/);
    const set = (f, val) => { const el = $(`[data-f="${f}"]`, body); if (el && val) el.value = val.trim(); };
    if (pts.length >= 2) { set('artist', pts[0]); set('title', pts.slice(1).join(' - ')); } else set('title', pts[0]);
    const num = stem.match(/^\s*(\d{1,3})\s*[-.)_ ]/);
    if (num) set('track', String(+num[1]));
    changed();
  };
  $('#e-case').onclick = () => {
    ['title', 'artist', 'album', 'albumArtist'].forEach((f) => {
      const el = $(`[data-f="${f}"]`, body);
      if (el?.value && (el.value === el.value.toUpperCase() || el.value === el.value.toLowerCase()))
        el.value = el.value.toLowerCase().replace(/(^|[\s(\[\-"/])(\p{L})/gu, (m, a, b) => a + b.toUpperCase());
    });
    changed();
  };
}
function tabCover(x, body, foot) {
  const min = S.settings.minCoverSize || 500;
  body.innerHTML = `<div class="bigcover" id="bigc">${icon('image')}</div>
    <div class="row wrap" style="margin:12px 0 4px">
      <span class="t-title-s">${x.cover ? `${x.cover.width}×${x.cover.height} px` : esc(t('noCover'))}</span>
      <span class="t-body-s muted grow">${x.cover ? `${esc(x.cover.mime.replace('image/', '').toUpperCase())} · ${fmtSize(x.cover.bytes)}` : ''}</span>
      ${x.cover ? (x.cover.width < min ? `<span class="tag warn">${esc(t('coverSmall'))}</span>` : `<span class="tag ok">${esc(t('coverOk'))}</span>`) : ''}</div>
    <div class="sec-title">${icon('image')}${esc(t('changeCover'))}</div>
    <div class="row wrap"><button class="btn tonal sm has-ic" data-act="cover-search">${icon('search')}${esc(t('coverSearch'))}</button>
      ${isAndroid() ? '' : `<button class="btn outlined sm has-ic" data-act="cover-file">${icon('upload')}${esc(t('coverFile'))}</button>`}
      <button class="btn outlined sm has-ic" data-act="cover-url">${icon('link')}${esc(t('coverUrl'))}</button></div>
    <p class="t-body-s muted" style="margin-top:14px">${esc(t('coverHint', { max: S.settings.maxCoverSize || 1200 }))}</p>`;
  foot.innerHTML = x.cover ? `${isAndroid() ? '' : `<button class="btn text has-ic" data-act="cover-export">${icon('download')}${esc(t('coverExport'))}</button>`}<span class="grow"></span><button class="btn text danger has-ic" data-act="cover-remove">${icon('trash')}${esc(t('coverRemove'))}</button>` : '';
  if (x.cover) invoke('get_cover', { id: x.id, size: 1200 }).then((u) => { const el = $('#bigc'); if (u && el) el.innerHTML = `<img src="${u}" alt="">`; });
}

// Tekst + karaoke
function parseLrc(text) {
  const out = [];
  for (const line of String(text || '').split(/\r?\n/)) {
    const tg = [...line.matchAll(/\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]/g)];
    if (!tg.length) continue;
    const words = line.replace(/\[[^\]]*\]/g, '').trim();
    for (const m of tg) out.push({ t: +m[1] * 60 + +m[2] + (m[3] ? +m[3] / Math.pow(10, m[3].length) : 0), text: words });
  }
  return out.sort((a, b) => a.t - b.t);
}
function shiftLrc(text, delta) {
  return text.replace(/\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]/g, (m, mm, ss, fr) => {
    const tt = Math.max(0, +mm * 60 + +ss + (fr ? +fr / Math.pow(10, fr.length) : 0) + delta);
    const M = Math.floor(tt / 60);
    return `[${String(M).padStart(2, '0')}:${(tt - M * 60).toFixed(2).padStart(5, '0')}]`;
  });
}
const audio = () => $('#player');
function stopAudio() { const a = audio(); if (a && !a.paused) a.pause(); }
function tabLyrics(x, body, foot) {
  const text = x.lyrics || '';
  const lines = parseLrc(text);
  body.innerHTML = `<div class="row wrap" style="margin-bottom:12px">${x.lyrics ? (x.lyricsSynced ? `<span class="tag ok">${icon('clock')}${esc(t('synced'))}</span>` : `<span class="tag warn">${esc(t('plain'))}</span>`) : `<span class="tag bad">${esc(t('noLyricsTag'))}</span>`}
      ${x.lrcFile ? '<span class="tag">.lrc</span>' : ''}<div class="grow"></div><button class="btn tonal sm has-ic" data-act="lyrics-search">${icon('search')}${esc(t('lyricsSearch'))}</button></div>
    ${lines.length ? `<div class="karaoke"><div class="lines" id="klines">${lines.map((l, i) => `<div class="ln" data-i="${i}" data-t="${l.t}">${esc(l.text || '♪')}</div>`).join('')}</div></div>` : ''}
    <div class="player"><button class="ibtn tonal" id="l-play">${icon('play')}</button><input type="range" id="l-seek" min="0" max="${Math.round(x.duration) || 1}" step="0.1" value="0"><span class="time" id="l-time">0:00 / ${fmtDur(x.duration)}</span></div>
    ${lines.length ? `<div class="row wrap" style="margin-bottom:12px"><span class="t-body-s muted">${esc(t('offset'))}</span><div class="seg sm"><button data-shift="-0.5">−0.5 s</button><button data-shift="-0.1">−0.1 s</button><button data-shift="0.1">+0.1 s</button><button data-shift="0.5">+0.5 s</button></div></div>` : ''}
    <div class="tf"><label>${esc(t('lyricsEditor'))}</label><textarea class="ta" id="l-text" spellcheck="false" placeholder="${esc(t('lyricsPh'))}">${esc(text)}</textarea></div>
    <p class="t-body-s muted">${esc(t('lyricsHint'))}</p>`;
  foot.innerHTML = `${x.lyrics ? `<button class="btn text danger has-ic" id="l-del">${icon('trash')}${esc(t('remove'))}</button>` : ''}<span class="grow"></span>
    <button class="btn outlined has-ic" id="l-stamp" title="${esc(t('stampHint'))}">${icon('clock')}${esc(t('stamp'))}</button><button class="btn filled has-ic" id="l-save" disabled>${icon('check')}${esc(t('save'))}</button>`;
  const ta = $('#l-text');
  ta.oninput = () => { $('#l-save').disabled = ta.value === text; };
  const a = audio();
  if (a.dataset.src !== x.path) { a.src = fileSrc(x.path); a.dataset.src = x.path; }
  const playBtn = $('#l-play'), seek = $('#l-seek'), time = $('#l-time'), lineEls = $$('.ln', body);
  let last = -1;
  const tick = () => {
    if (!document.body.contains(seek)) return;
    seek.value = a.currentTime;
    time.textContent = `${fmtDur(a.currentTime)} / ${fmtDur(x.duration)}`;
    if (!lines.length) return;
    let idx = -1;
    for (let i = 0; i < lines.length; i++) if (lines[i].t <= a.currentTime + 0.15) idx = i; else break;
    if (idx === last) return;
    last = idx;
    lineEls.forEach((el, i) => { el.classList.toggle('cur', i === idx); el.classList.toggle('past', i < idx); });
    const cur = lineEls[Math.max(0, idx)];
    if (cur) $('#klines').style.transform = `translateY(${-cur.offsetTop + 120}px)`;
  };
  a.ontimeupdate = tick;
  a.onplay = () => { playBtn.innerHTML = icon('pause'); };
  a.onpause = () => { playBtn.innerHTML = icon('play'); };
  a.onerror = () => { playBtn.disabled = true; playBtn.title = t('cantPlay'); };
  playBtn.onclick = () => (a.paused ? a.play().catch((e) => snack(t('cantPlay') + ': ' + errMsg(e), 'bad')) : a.pause());
  seek.oninput = () => { a.currentTime = +seek.value; tick(); };
  lineEls.forEach((el) => { el.onclick = () => { a.currentTime = +el.dataset.t; tick(); if (a.paused) a.play().catch(() => {}); }; });
  $$('[data-shift]', body).forEach((b) => { b.onclick = () => { ta.value = shiftLrc(ta.value, +b.dataset.shift); $('#l-save').disabled = false; snack(t('shifted', { s: b.dataset.shift })); }; });
  $('#l-stamp').onclick = () => {
    const pos = ta.selectionStart;
    const start = ta.value.lastIndexOf('\n', pos - 1) + 1;
    const tt = a.currentTime, M = Math.floor(tt / 60);
    const stamp = `[${String(M).padStart(2, '0')}:${(tt - M * 60).toFixed(2).padStart(5, '0')}]`;
    const rest = ta.value.slice(start).replace(/^\[\d{1,3}:\d{2}(?:[.:]\d{1,3})?\]/, '');
    ta.value = ta.value.slice(0, start) + stamp + rest;
    const next = ta.value.indexOf('\n', start);
    const np = next < 0 ? ta.value.length : next + 1;
    ta.focus();
    ta.setSelectionRange(np, np);
    $('#l-save').disabled = false;
  };
  $('#l-save').onclick = async () => {
    try { await invoke('save_edits', { req: { ids: [x.id], lyrics: ta.value.trim() || null } }); snack(t('saved')); } catch (e) { snack(errMsg(e), 'bad'); }
  };
  if ($('#l-del')) $('#l-del').onclick = async () => {
    if (!(await confirmBox(t('remove'), t('lyricsRemoveQ'), t('remove'), true))) return;
    try { await invoke('save_edits', { req: { ids: [x.id], lyrics: null } }); snack(t('saved')); } catch (e) { snack(errMsg(e), 'bad'); }
  };
}
function tabFile(x, body, foot) {
  const kv = [
    [t('fPath'), x.path], [t('fFormat'), x.format], [t('fSize'), fmtSize(x.size)], [t('fDuration'), fmtDur(x.duration)],
    [t('fBitrate'), x.bitrate ? x.bitrate + ' kbps' : '—'], [t('fSampleRate'), x.sampleRate ? (x.sampleRate / 1000).toFixed(1) + ' kHz' : '—'],
    [t('fChannels'), x.channels ?? '—'], [t('fCover'), x.cover ? `${x.cover.width}×${x.cover.height}` : '—'],
    [t('fLyrics'), x.lyrics ? (x.lyricsSynced ? t('synced') : t('plain')) + (x.lrcFile ? ' + .lrc' : '') : '—'],
    [t('fModified'), x.modified ? new Date(x.modified * 1000).toLocaleString(currentLang()) : '—'], ['MusicBrainz', x.tags.mbRecordingId || '—'],
  ];
  body.innerHTML = `<div class="kv">${kv.map(([k, v]) => `<div class="k">${esc(k)}</div><div class="v">${esc(v)}</div>`).join('')}</div>
    <div class="sec-title">${icon('alert')}${esc(t('completeness'))}</div>
    <div class="row wrap">${x.missing.length ? x.missing.map((m) => `<span class="tag warn">${esc(t('mi_' + m))}</span>`).join('') : `<span class="tag ok">${esc(t('allComplete'))}</span>`}</div>`;
  foot.innerHTML = `${!isAndroid() ? `<button class="btn text has-ic" data-act="reveal">${icon('folderOpen')}${esc(t('showInFolder'))}</button>` : ''}
    ${x.tags.mbRecordingId ? `<button class="btn text has-ic" data-act="open-mb">${icon('external')}MusicBrainz</button>` : ''}<span class="grow"></span>
    <button class="btn text has-ic" data-act="reload-track">${icon('refresh')}${esc(t('reload'))}</button>`;
}

// ─────────────── Weryfikacja ───────────────
function reviewList() {
  const words = fold(S.rv.q).split(/\s+/).filter(Boolean);
  const th = S.settings.autoApplyThreshold ?? 0.9;
  return pendingProposals().filter((x) => {
    const p = S.proposals.get(x.id);
    if (S.rv.conf === 'sure' && p.confidence < th) return false;
    if (S.rv.conf === 'check' && p.confidence >= th) return false;
    if (S.rv.types.has('tags') && !p.changes.some((c) => c.enabled)) return false;
    if (S.rv.types.has('cover') && !p.coverEnabled) return false;
    if (S.rv.types.has('lyrics') && !p.lyricsEnabled) return false;
    if (S.rv.types.has('fingerprint') && p.method !== 'fingerprint') return false;
    if (words.length) {
      const nt = p.changes.find((c) => c.field === 'title')?.new;
      const na = p.changes.find((c) => c.field === 'artist')?.new;
      const hay = fold([x.fileName, x.tags.title, x.tags.artist, nt, na].join(' '));
      if (!words.every((w) => hay.includes(w))) return false;
    }
    return true;
  }).sort((a, b) => S.proposals.get(b.id).confidence - S.proposals.get(a.id).confidence || (a.path < b.path ? -1 : 1));
}
const newVal = (p, f, x) => p.changes.find((c) => c.field === f && c.enabled)?.new ?? x.tags[f];
function renderReview(v) {
  const all = pendingProposals();
  if (!all.length) {
    const need = S.tracks.filter((x) => x.score < 100).length;
    const queued = S.jobs.apply?.running;
    v.innerHTML = `<div class="empty">${icon(queued ? 'download' : 'sparkles', 'xl')}<h2>${esc(t(queued ? 'reviewDoneTitle' : 'reviewEmptyTitle'))}</h2><p>${esc(S.tracks.length ? t('reviewEmpty', { n: need }) : t('scanFirst'))}</p>
      ${S.tracks.length && need ? `<button class="btn filled has-ic" data-act="analyze-all">${icon('sparkles')}${esc(t('smartFixN', { n: need }))}</button>` : `<button class="btn tonal" data-nav="library">${esc(t('goLibrary'))}</button>`}</div>`;
    return;
  }
  document.body.classList.add('has-actions');
  v.innerHTML = `<div class="review ${S.rv.detail ? 'detail' : ''}" id="review"><div class="rv-list" id="rvlist"></div><div class="rv-pane" id="rvpane"></div></div>`;
  refreshReview();
}
function refreshReview() {
  if (S.view !== 'review') return;
  const all = pendingProposals();
  if (!all.length || !$('#review')) return renderView();
  const list = reviewList();
  if (!S.rv.active || !list.some((x) => x.id === S.rv.active)) S.rv.active = list[0]?.id || null;
  renderReviewList(list, all);
  renderReviewPane(list);
}
function renderReviewList(list, all) {
  const th = S.settings.autoApplyThreshold ?? 0.9;
  const nSure = all.filter((x) => S.proposals.get(x.id).confidence >= th).length;
  for (const id of [...S.rv.sel]) if (!S.proposals.has(id) || S.byId.get(id)?.status !== 'proposed') S.rv.sel.delete(id);
  const type = (k, ic) => `<button class="chip ${S.rv.types.has(k) ? 'on' : ''}" data-rvtype="${k}">${icon('check', 'chk')}${S.rv.types.has(k) ? '' : icon(ic)}${esc(t('rt_' + k))}</button>`;
  const el = $('#rvlist');
  const scroll = $('.rv-items', el)?.scrollTop || 0;
  const qFocused = document.activeElement?.id === 'rv-q';
  el.innerHTML = `<div class="tools">
      <div class="search sm">${icon('search')}<input id="rv-q" placeholder="${esc(t('searchPh'))}" value="${esc(S.rv.q)}"></div>
      <div class="seg sm" style="align-self:flex-start"><button class="${S.rv.conf === 'all' ? 'on' : ''}" data-rvconf="all">${esc(t('rcAll'))} ${all.length}</button><button class="${S.rv.conf === 'sure' ? 'on' : ''}" data-rvconf="sure">${esc(t('rcSure'))} ${nSure}</button><button class="${S.rv.conf === 'check' ? 'on' : ''}" data-rvconf="check">${esc(t('rcCheck'))} ${all.length - nSure}</button></div>
      <div class="chips">${type('tags', 'tag')}${type('cover', 'image')}${type('lyrics', 'lyrics')}${type('fingerprint', 'fingerprint')}</div>
    </div>
    ${S.rv.sel.size ? `<div class="rv-bulk"><span class="grow t-title-s">${esc(t('selected', { n: S.rv.sel.size }))}</span><button class="btn text sm" data-act="rv-sel-reject">${esc(t('reject'))}</button><button class="btn filled sm" data-act="rv-sel-accept">${esc(t('accept'))}</button><button class="ibtn" data-act="rv-sel-clear">${icon('x', 'sm')}</button></div>` : ''}
    <div class="rv-items">${list.length ? list.map((x) => {
      const p = S.proposals.get(x.id);
      const sub = [newVal(p, 'artist', x), newVal(p, 'album', x)].filter(Boolean).join(' · ') || x.fileName;
      return `<div class="rv-item ${S.rv.active === x.id ? 'active' : ''}" data-rv="${x.id}">
        <span class="cbw" data-rvsel="${x.id}"><span class="cb ${S.rv.sel.has(x.id) ? 'on' : ''}">${icon('check')}</span></span>
        ${coverHTML(x)}
        <div class="grow"><div class="t1 ellipsis">${esc(newVal(p, 'title', x) || x.fileName)}</div><div class="t2 ellipsis">${esc(sub)}</div>
          <div class="row" style="gap:6px;margin-top:2px;opacity:.8">${p.changes.some((c) => c.enabled) ? icon('tag', 'sm') : ''}${p.coverEnabled ? icon('image', 'sm') : ''}${p.lyricsEnabled ? icon('lyrics', 'sm') : ''}</div></div>
        <span class="conf ${confCls(p.confidence)}">${Math.round(p.confidence * 100)}%</span></div>`;
    }).join('') : `<div class="empty" style="padding:40px 16px">${icon('filter', 'xl')}<p>${esc(t('nothingHere'))}</p></div>`}</div>`;
  $('.rv-items', el).scrollTop = scroll;
  if (qFocused) { const i = $('#rv-q'); i.focus(); i.setSelectionRange(i.value.length, i.value.length); }
}
function renderReviewPane(list) {
  const pane = $('#rvpane');
  const x = S.byId.get(S.rv.active);
  const p = x && S.proposals.get(x.id);
  if (!x || !p) { pane.innerHTML = `<div class="empty">${icon('sparkles', 'xl')}<p>${esc(t('pickProposal'))}</p></div>`; return; }
  const idx = list.findIndex((y) => y.id === x.id);
  const alt = p.alternatives || [];
  const nOn = p.changes.filter((c) => c.enabled).length;
  const min = S.settings.minCoverSize || 500;
  const prevBody = $('.rv-body', pane);
  const scroll = prevBody?.dataset.id === x.id ? prevBody.scrollTop : 0;
  const nothing = !p.changes.length && !p.covers.length && !p.lyrics;
  const curUrl = x.cover ? S.covers.get(x.id + ':300') : null;
  if (x.cover && curUrl === undefined) wantCover(x.id, 300);
  pane.innerHTML = `<div class="rv-body" data-id="${x.id}">
    ${isMobile() ? `<button class="btn text has-ic" data-act="rv-back" style="margin-left:-12px">${icon('chevronLeft')}${esc(t('backToList'))}</button>` : ''}
    <div class="rv-head">${coverHTML(x, 300, 'cover lg')}
      <div class="grow"><h2>${esc(newVal(p, 'title', x) || x.fileName)}</h2>
        <div class="muted">${esc([newVal(p, 'artist', x), newVal(p, 'album', x), newVal(p, 'year', x)].filter(Boolean).join(' · '))}</div>
        <div class="t-body-s dim ellipsis" style="margin-top:4px" title="${esc(x.path)}">${esc(x.fileName)}</div>
        <div class="row wrap" style="margin-top:10px"><span class="conf ${confCls(p.confidence)}">${esc(t('confidenceN', { p: Math.round(p.confidence * 100) }))}</span><span class="tag">${icon(p.method === 'fingerprint' ? 'fingerprint' : 'search')}${esc(t('m_' + p.method))}</span>
          ${alt.length > 1 ? `<button class="btn text sm has-ic" id="rv-alts">${icon('layers')}${esc(t('altN', { n: alt.length }))}</button>` : ''}</div></div></div>
    ${x.error ? `<div class="banner err">${icon('alert')}<span class="selectable t-body-s">${esc(x.error)}</span></div>` : ''}
    ${nothing ? `<p class="muted">${esc(t('nothingToChange'))}</p>` : ''}
    ${p.changes.length ? `<div class="sec-title">${icon('tag')}<span class="grow">${esc(t('tagChanges'))} <span class="muted">${nOn}/${p.changes.length}</span></span><button class="btn text sm" id="rv-all-fields">${esc(nOn === p.changes.length ? t('noneFields') : t('allFields'))}</button></div>
      <div class="diff-wrap"><table class="diff"><tbody>${p.changes.map((c, i) => `<tr class="${c.enabled ? '' : 'off'}">
        <td class="cbc"><span class="cbw" data-chcb="${i}"><span class="cb ${c.enabled ? 'on' : ''}">${icon('check')}</span></span></td>
        <td class="k">${esc(fieldLabel(c.field))}</td>
        <td class="new"><span contenteditable="true" spellcheck="false" data-edit="${i}">${esc(c.new ?? '')}</span></td>
        <td class="old ${c.old ? '' : 'none'}">${c.old ? esc(c.old) : esc(t('emptyVal'))}</td></tr>`).join('')}</tbody></table></div>` : ''}
    ${p.covers.length ? `<div class="sec-title">${icon('image')}<span class="grow">${esc(t('coverSec'))}</span><span class="t-body-s muted">${esc(p.coverEnabled ? (x.cover ? t('willReplace') : t('willAdd')) : t('willKeep'))}</span><button class="switch ${p.coverEnabled ? 'on' : ''}" id="rv-cover-on"></button></div>
      <div class="covers"><div class="cv current"><div class="img" ${x.cover ? `data-cover="${x.id}:300"` : ''}>${curUrl ? `<img src="${curUrl}" alt="">` : icon(x.cover ? 'disc' : 'image')}</div>
          <div class="src">${esc(t('currentCover'))}</div><div class="res ${x.cover && x.cover.width < min ? 'low' : ''}">${x.cover ? `${x.cover.width}×${x.cover.height} px` : esc(t('none'))}</div></div>
        <div class="arrow-sep">${icon('chevronRight', 'lg')}</div>
        ${p.covers.map((c, i) => { coverRes(c.url); return `<div class="cv ${p.coverEnabled && p.coverChoice === i ? 'on' : ''}" data-cpick="${i}"><span class="check">${icon('check')}</span><div class="img"><img src="${esc(c.thumb)}" alt="" loading="lazy" onerror="this.closest('.cv').style.display='none'"></div>
          <div class="src ellipsis" title="${esc(c.source)}">${esc(c.source)}</div>${resHTML(c.url)}</div>`; }).join('')}</div>` : ''}
    ${p.lyrics ? `<div class="sec-title">${icon('lyrics')}<span class="grow">${esc(t('lyricsSec'))} ${p.lyrics.synced ? `<span class="tag ok">${icon('clock')}${esc(t('synced'))}</span>` : `<span class="tag">${esc(t('plain'))}</span>`}</span>
        ${p.lyrics.instrumental ? '' : `<span class="t-body-s muted">${esc(p.lyricsEnabled ? (x.lyrics ? t('willReplace') : t('willAdd')) : t('willSkip'))}</span><button class="switch ${p.lyricsEnabled ? 'on' : ''}" id="rv-lyr-on"></button>`}</div>
      ${p.lyrics.instrumental ? `<p class="muted">${esc(t('instrumental'))}</p>` : `<div class="lyr-prev">${esc(p.lyrics.synced || p.lyrics.plain || '')}</div>`}` : ''}
    ${(p.notes || []).filter((n) => n !== 'instrumental').length ? `<div class="sec-title">${icon('info')}${esc(t('notes'))}</div><div class="t-body-s muted selectable">${p.notes.filter((n) => n !== 'instrumental').map(esc).join('<br>')}</div>` : ''}
    </div>
    <div class="rv-actions">
      <button class="ibtn" data-act="rv-prev" title="←" ${idx > 0 ? '' : 'disabled'}>${icon('chevronLeft')}</button>
      <span class="t-body-s muted count" style="min-width:56px;text-align:center">${idx + 1} / ${list.length}</span>
      <button class="ibtn" data-act="rv-next" title="→" ${idx < list.length - 1 ? '' : 'disabled'}>${icon('chevronRight')}</button>
      <span class="grow"></span>
      <button class="btn text has-ic" data-act="rv-edit" title="E">${icon('edit')}<span class="lbl-m">${esc(t('details'))}</span></button>
      <button class="btn text danger" data-act="rv-reject" title="D / Delete">${esc(t('reject'))}<span class="kbd">D</span></button>
      <button class="btn outlined" data-act="rv-skip" title="S">${esc(t('skip'))}<span class="kbd">S</span></button>
      <button class="btn filled has-ic" data-act="rv-accept" title="A / Enter" ${nothing ? 'disabled' : ''}>${icon('check')}${esc(t('accept'))}<span class="kbd">A</span></button>
    </div>`;
  $('.rv-body', pane).scrollTop = scroll;
  const save = debounce(() => invoke('update_proposal', { proposal: p }), 200);
  const rerender = () => { save(); renderReviewPane(list); renderReviewList(list, pendingProposals()); };
  $$('[data-chcb]', pane).forEach((el) => { el.onclick = () => { const c = p.changes[+el.dataset.chcb]; c.enabled = !c.enabled; rerender(); }; });
  $$('[data-edit]', pane).forEach((el) => {
    el.addEventListener('input', () => { const c = p.changes[+el.dataset.edit]; c.new = el.textContent.trim() || null; c.enabled = true; el.closest('tr').classList.remove('off'); $('.cb', el.closest('tr')).classList.add('on'); save(); });
    el.addEventListener('keydown', (e) => { e.stopPropagation(); if (e.key === 'Enter' || e.key === 'Escape') { e.preventDefault(); el.blur(); } });
  });
  if ($('#rv-all-fields', pane)) $('#rv-all-fields', pane).onclick = () => { const on = nOn !== p.changes.length; p.changes.forEach((c) => (c.enabled = on)); rerender(); };
  if ($('#rv-cover-on', pane)) $('#rv-cover-on', pane).onclick = () => { p.coverEnabled = !p.coverEnabled; if (p.coverChoice == null) p.coverChoice = 0; rerender(); };
  if ($('#rv-lyr-on', pane)) $('#rv-lyr-on', pane).onclick = () => { p.lyricsEnabled = !p.lyricsEnabled; rerender(); };
  $$('[data-cpick]', pane).forEach((el) => {
    el.onclick = () => { const i = +el.dataset.cpick; if (p.coverEnabled && p.coverChoice === i) p.coverEnabled = false; else { p.coverChoice = i; p.coverEnabled = true; } rerender(); };
  });
  if ($('#rv-alts', pane)) $('#rv-alts', pane).onclick = (e) => menu(e.currentTarget, alt.map((a, i) => ({
    icon: p.chosen === i ? 'check' : 'disc',
    label: `${a.fields.title || ''} — ${[a.fields.album, a.fields.year, a.secondaryTypes?.join('/')].filter(Boolean).join(' · ')}`,
    end: `${a.source} · ${Math.round(a.score * 100)}%`,
    run: async () => {
      try { const np = await invoke('choose_alternative', { id: x.id, index: i }); applySkipFields(np); S.proposals.set(x.id, np); refreshReview(); } catch (err) { snack(errMsg(err), 'bad'); }
    },
  })));
}
function reviewStep(dir) {
  const list = reviewList();
  const i = list.findIndex((x) => x.id === S.rv.active);
  const n = list[i + dir];
  if (n) { S.rv.active = n.id; refreshReview(); scrollReviewItem(); }
}
function scrollReviewItem() { $(`.rv-item[data-rv="${S.rv.active}"]`)?.scrollIntoView({ block: 'nearest' }); }
let rvDetailLayer = null;
function openReviewDetail() {
  if (!isMobile() || S.rv.detail) return;
  S.rv.detail = true;
  $('#review')?.classList.add('detail');
  rvDetailLayer = (fromBack) => closeReviewDetail(fromBack);
  pushLayer(rvDetailLayer);
}
function closeReviewDetail(fromBack) {
  if (!S.rv.detail) return;
  S.rv.detail = false;
  $('#review')?.classList.remove('detail');
  if (!fromBack && rvDetailLayer) dropLayer(rvDetailLayer);
  rvDetailLayer = null;
}
/** Decyzja dla aktywnej propozycji i przejście do następnej. */
async function reviewDecide(kind) {
  const list = reviewList();
  const i = list.findIndex((x) => x.id === S.rv.active);
  const x = list[i];
  if (!x) return;
  if (kind === 'skip') {
    if (i === list.length - 1) return snack(t('lastProposal'));
    return reviewStep(1);
  }
  const next = list[i + 1] || list[i - 1];
  if (kind === 'accept') await acceptIds([x.id], true);
  else await rejectIds([x.id], true);
  S.rv.active = next && next.id !== x.id ? next.id : null;
  refreshReview();
  renderTopbar();
  renderNav();
  setTimeout(scrollReviewItem, 30);
}
async function acceptIds(ids, quiet) {
  ids = ids.filter((id) => S.proposals.has(id));
  if (!ids.length) return snack(t('nothingToApply'));
  for (const id of ids) { const x = S.byId.get(id); if (x) x.status = 'queued'; S.rv.sel.delete(id); }
  try {
    await invoke('apply_proposals', { ids });
    if (!quiet) snack(t('acceptedN', { n: ids.length }));
  } catch (e) {
    for (const id of ids) { const x = S.byId.get(id); if (x) x.status = 'proposed'; }
    snack(errMsg(e), 'bad');
  }
  schedule('review');
  schedule('rows');
}
async function rejectIds(ids, quiet) {
  if (!ids.length) return;
  await invoke('discard_proposals', { ids });
  for (const id of ids) { S.proposals.delete(id); const x = S.byId.get(id); if (x) x.status = 'idle'; S.rv.sel.delete(id); }
  if (!quiet) snack(t('rejectedN', { n: ids.length }));
  schedule('review');
  schedule('rows');
}
function reviewMore(anchor) {
  const vis = reviewList().map((x) => x.id);
  const fields = ['title', 'artist', 'album', 'albumArtist', 'year', 'track', 'trackTotal', 'disc', 'discTotal', 'genre', 'label', 'isrc', 'bpm', 'composer', 'mbRecordingId', 'mbReleaseId', 'cover', 'lyrics'];
  menu(anchor, [
    { icon: 'checkAll', label: t('acceptVisible', { n: vis.length }), run: async () => { if (await confirmBox(t('acceptVisibleQ'), t('acceptVisibleText', { n: vis.length }), t('accept'))) acceptIds(vis); } },
    { icon: 'x', label: t('rejectVisible', { n: vis.length }), run: async () => { if (await confirmBox(t('rejectVisible', { n: vis.length }), t('rejectQ'), t('reject'), true)) rejectIds(vis); } },
    'sep',
    { header: t('skipFields') },
    ...fields.map((f) => ({
      label: f === 'cover' ? t('coverSec') : f === 'lyrics' ? t('lyricsSec') : fieldLabel(f),
      switch: S.rv.skip.has(f),
      run: (on) => {
        on ? S.rv.skip.add(f) : S.rv.skip.delete(f);
        store('skipFields', JSON.stringify([...S.rv.skip]));
        for (const x of pendingProposals()) {
          const p = S.proposals.get(x.id);
          if (on) applySkipFields(p);
          else {
            const c = p.changes.find((y) => y.field === f);
            if (c) c.enabled = true;
            if (f === 'cover' && p.covers.length) p.coverEnabled = true;
            if (f === 'lyrics' && p.lyrics && !p.lyrics.instrumental) p.lyricsEnabled = true;
            invoke('update_proposal', { proposal: p });
          }
        }
        schedule('review');
      },
    })),
  ]);
}

// ─────────────── Historia ───────────────
async function loadHistory() {
  try { S.history = await invoke('get_history'); } catch (_) { S.history = []; }
  if (S.view === 'history') { renderView(); renderTopbar(); }
}
function renderHistory(v) {
  const h = S.history || [];
  const batches = [], map = new Map();
  for (const e of h) {
    if (!map.has(e.batch)) { const b = { id: e.batch, time: e.time, items: [] }; map.set(e.batch, b); batches.push(b); }
    map.get(e.batch).items.push(e);
  }
  const sumText = (e) => (e.action === 'rename' ? e.summary.join(', ') : e.summary.map((s) => (t('f_' + s) !== 'f_' + s ? t('f_' + s) : t('mi_' + s))).join(', '));
  v.innerHTML = `<div class="page"><div class="page-inner">
    ${!h.length ? `<div class="empty">${icon('history', 'xl')}<h2>${esc(t('historyEmptyTitle'))}</h2><p>${esc(t('historyEmpty'))}</p></div>` : ''}
    ${batches.slice(0, 200).map((b) => {
      const active = b.items.filter((x) => !x.undone).length;
      return `<div class="list-card"><div class="li-row">${icon('layers')}<div class="txt"><b>${esc(t('batchLabel', { n: b.items.length }))}</b><span>${new Date(b.time * 1000).toLocaleString(currentLang())}</span></div>
        ${active ? `<button class="btn tonal sm has-ic" data-undo="${esc(b.id)}">${icon('undo')}${esc(t('undoBatch'))}</button>` : `<span class="tag">${esc(t('undone'))}</span>`}</div>
        ${b.items.slice(0, 50).map((e) => `<div class="li-row ${e.undone ? 'undone' : ''}">${icon(e.action === 'rename' ? 'rename' : e.action === 'edit' ? 'edit' : 'sparkles', 'dim')}
          <div class="txt"><b class="ellipsis">${esc(baseName(e.path))}</b><span class="ellipsis">${esc(sumText(e))}</span></div>
          ${e.undone ? '' : `<button class="ibtn" data-undo="${esc(e.id)}" title="${esc(t('undo'))}">${icon('undo')}</button>`}</div>`).join('')}
        ${b.items.length > 50 ? `<div class="li-row muted">+${b.items.length - 50}</div>` : ''}</div>`;
    }).join('')}</div></div>`;
}
async function undoIds(ids) {
  try { const n = await invoke('undo', { ids }); snack(t('undoneN', { n })); loadHistory(); } catch (e) { snack(errMsg(e), 'bad'); }
}
async function undoLast() {
  const h = await invoke('get_history');
  const last = h.find((e) => !e.undone);
  if (!last) return snack(t('historyEmpty'));
  undoIds([last.batch]);
}

// ─────────────── Ustawienia ───────────────
function renderSettings(v) {
  const s = S.settings;
  const sw = (k) => `<button class="switch ${s[k] ? 'on' : ''}" role="switch" aria-checked="${!!s[k]}" tabindex="-1"></button>`;
  const row = (title, desc, ctrl, k) => `<div class="li-row ${k ? 'click' : ''}" ${k ? `data-swrow="${k}"` : ''}><div class="txt"><b>${esc(title)}</b>${desc ? `<span>${desc}</span>` : ''}</div>${ctrl}</div>`;
  const swRow = (k, title, desc = '') => row(title, desc ? esc(desc) : '', sw(k), k);
  const num = (k, min, max, step, fmt) => `<div class="range-wrap"><input type="range" data-num="${k}" min="${min}" max="${max}" step="${step}" value="${s[k]}"><b data-numv="${k}">${fmt(s[k])}</b></div>`;
  const sel = (k, opts) => `<select class="sel" data-sel="${k}">${opts.map(([val, l]) => `<option value="${val}" ${String(s[k]) === String(val) ? 'selected' : ''}>${esc(l)}</option>`).join('')}</select>`;
  const px = (n) => n + ' px';
  const ACC = { violet: '#6750a4', blue: '#0061a4', green: '#006e1c', orange: '#8b5000', pink: '#984061' };
  v.innerHTML = `<div class="page"><div class="page-inner">
    <div class="list-card"><h2>${icon('sun')}${esc(t('sAppearance'))}</h2>
      ${row(t('theme'), '', sel('theme', [['dark', t('themeDark')], ['light', t('themeLight')], ['auto', t('themeAuto')]]))}
      ${row(t('accent'), '', `<div class="swatches">${Object.entries(ACC).map(([a, c]) => `<button class="swatch ${s.accent === a ? 'on' : ''}" data-accent-pick="${a}" style="background:${c}">${icon('check')}</button>`).join('')}</div>`)}
      ${row(t('language'), '', sel('language', [['auto', t('langAuto')], ['pl', 'Polski'], ['en', 'English']]))}</div>
    <div class="list-card"><h2>${icon('folderOpen')}${esc(t('sScan'))}</h2>${swRow('recursive', t('recursive'), t('recursiveHint'))}</div>
    <div class="list-card"><h2>${icon('sparkles')}${esc(t('sBehavior'))}</h2>
      ${row(t('writeMode'), esc(t('writeModeD')), sel('writeMode', [['missing', t('wmMissing')], ['overwrite', t('wmOverwrite')]]))}
      ${swRow('autoApply', t('autoApply'), t('autoApplyD'))}
      ${row(t('threshold'), esc(t('thresholdD')), num('autoApplyThreshold', 0.6, 1, 0.01, (x) => Math.round(x * 100) + '%'))}
      ${swRow('preferOriginalRelease', t('preferOriginal'), t('preferOriginalD'))}
      ${swRow('fillAlbumArtist', t('fillAlbumArtist'))}${swRow('fillGenre', t('fillGenre'))}${swRow('writeMbIds', t('writeMbIds'), t('writeMbIdsD'))}
      ${row(t('concurrency'), esc(t('concurrencyD')), num('concurrency', 1, 8, 1, (x) => x))}</div>
    <div class="list-card"><h2>${icon('image')}${esc(t('sCovers'))}</h2>
      ${swRow('fetchCovers', t('fetchCovers'))}${swRow('replaceSmallCovers', t('replaceSmall'), t('replaceSmallD'))}
      ${row(t('minCover'), '', num('minCoverSize', 200, 1000, 50, px))}${row(t('maxCover'), esc(t('maxCoverD')), num('maxCoverSize', 500, 3000, 100, px))}
      ${row(t('coverQuality'), '', num('coverQuality', 60, 100, 1, (x) => x + '%'))}${swRow('saveFolderJpg', t('saveFolderJpg'), t('saveFolderJpgD'))}</div>
    <div class="list-card"><h2>${icon('lyrics')}${esc(t('sLyrics'))}</h2>
      ${swRow('fetchLyrics', t('fetchLyrics'))}${swRow('preferSynced', t('preferSynced'), t('preferSyncedD'))}
      ${swRow('embedLyrics', t('embedLyrics'), t('embedLyricsD'))}${swRow('writeLrcFile', t('writeLrc'), t('writeLrcD'))}</div>
    <div class="list-card"><h2>${icon('layers')}${esc(t('sSources'))}</h2>
      ${swRow('useMusicbrainz', 'MusicBrainz + Cover Art Archive', t('srcMb'))}${swRow('useDeezer', 'Deezer', t('srcDeezer'))}${swRow('useItunes', 'Apple iTunes', t('srcItunes'))}
      ${row(t('itunesCountry'), '', sel('itunesCountry', [['PL', 'Polska'], ['US', 'USA'], ['GB', 'UK'], ['DE', 'Deutschland'], ['FR', 'France'], ['JP', 'Japan']]))}
      ${swRow('useLrclib', 'LRCLIB', t('srcLrclib'))}
      ${row('AcoustID', esc(t('srcAcoustid')) + ` <a href="#" id="acoustid-link">${esc(t('getKey'))}</a>`, sw('useAcoustid'), 'useAcoustid')}
      ${row(t('acoustidKey'), esc(t('acoustidKeyD')), `<input class="inp mono" data-text="acoustidKey" value="${esc(s.acoustidKey || '')}" placeholder="AbCdEf123">`)}</div>
    <div class="list-card"><h2>${icon('rename')}${esc(t('sRename'))}</h2>
      ${row(t('pattern'), esc(t('patternD')), `<input class="inp mono" data-text="renamePattern" value="${esc(s.renamePattern || '')}">`)}</div>
    <div class="list-card"><h2>${icon('info')}${esc(t('sAbout'))}</h2>
      ${row('MusicTagger ' + S.info.version, esc(t('aboutD')), `<button class="btn tonal sm has-ic" id="chk-upd">${icon('update')}${esc(t('checkUpdates'))}</button>`)}
      ${row(t('shortcuts'), esc(t('shortcutsD')), '')}
      ${row(t('dataDir'), `<span class="mono selectable">${esc(S.info.dataDir || '')}</span>`, '')}</div>
  </div></div>`;
  const persist = debounce(async () => {
    try { S.settings = await invoke('save_settings', { settings: S.settings }); } catch (e) { snack(errMsg(e), 'bad'); }
  }, 300);
  $$('[data-swrow]', v).forEach((r) => { r.onclick = (e) => { if (e.target.closest('a')) return; const k = r.dataset.swrow; s[k] = !s[k]; $('.switch', r).classList.toggle('on', s[k]); persist(); }; });
  $$('[data-num]', v).forEach((r) => {
    r.oninput = () => {
      const k = r.dataset.num;
      s[k] = k === 'autoApplyThreshold' ? +r.value : Math.round(+r.value);
      $(`[data-numv="${k}"]`, v).textContent = k === 'autoApplyThreshold' ? Math.round(s[k] * 100) + '%' : k.includes('Size') ? s[k] + ' px' : k === 'coverQuality' ? s[k] + '%' : s[k];
      persist();
    };
  });
  $$('[data-sel]', v).forEach((el) => {
    el.onchange = () => {
      const k = el.dataset.sel;
      s[k] = el.value;
      persist();
      if (k === 'theme') { applyTheme(); renderNav(); }
      if (k === 'language') { setLang(s.language); renderAll(); }
    };
  });
  $$('[data-text]', v).forEach((el) => { el.oninput = () => { s[el.dataset.text] = el.value.trim(); persist(); }; });
  $$('[data-accent-pick]', v).forEach((b) => { b.onclick = () => { s.accent = b.dataset.accentPick; applyTheme(); persist(); renderView(); }; });
  $('#acoustid-link').onclick = (e) => { e.preventDefault(); e.stopPropagation(); opener()?.openUrl('https://acoustid.org/new-application'); };
  $('#chk-upd').onclick = () => checkUpdate(false);
}
function applyTheme() {
  let th = S.settings.theme || 'dark';
  if (th === 'auto') th = matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
  document.documentElement.dataset.theme = th;
  document.documentElement.dataset.accent = S.settings.accent || 'violet';
}
async function checkUpdate(silent) {
  try {
    const u = await invoke('check_update');
    if (u.available) snack(t('updateAvail', { v: u.latest }), 'info', { label: t('download'), run: () => opener()?.openUrl(u.download || u.url) });
    else if (!silent) snack(t('upToDate'));
  } catch (e) { if (!silent) snack(errMsg(e), 'bad'); }
}

// ═════════════════════════════ Akcje ═════════════════════════════
async function pickFolder(append = false) {
  let path = null;
  const d = dialogApi();
  if (d && !isAndroid()) {
    try { path = await d.open({ directory: true, multiple: false, defaultPath: S.folder || undefined, title: t('openFolder') }); } catch (_) { path = null; }
  } else path = await folderBrowser();
  if (path) scanFolder(path, { append });
}
async function pickFiles(append = false) {
  const d = dialogApi();
  if (!d) return;
  let paths = await d.open({ multiple: true, directory: false, title: t('openFiles'), filters: [{ name: t('audioFiles'), extensions: ['mp3', 'flac', 'm4a', 'mp4', 'aac', 'ogg', 'oga', 'opus', 'wav', 'wv', 'ape', 'aiff', 'aif', 'mpc', 'spx'] }] }).catch(() => null);
  if (!paths) return;
  if (!Array.isArray(paths)) paths = [paths];
  runScan(paths.length === 1 ? baseName(paths[0]) : t('nFiles', { n: paths.length }), () => invoke('scan_files', { paths, append }), append);
}
function scanFolder(path, { append = false, recursive } = {}) {
  if (recursive !== undefined) S.settings.recursive = recursive;
  runScan(path, () => invoke('scan_folder', { path, recursive: S.settings.recursive !== false, append }), append);
}
async function runScan(label, call, append) {
  if (S.scan && S.scan.phase !== 'done') return snack(t('busy'));
  S.folder = append && S.folder ? S.folder : label;
  S.scan = { done: 0, total: 0, phase: 'collect' };
  if (!append) { closeSheet(); setTracks([]); }
  S.view = 'library';
  renderAll();
  try {
    const list = await call();
    S.scan = null;
    setTracks(list, append);
    S.settings = await invoke('get_settings');
    renderAll();
    if (list.length) {
      const need = list.filter((x) => x.score < 100).length;
      snack(t('scanned', { n: list.length, need }), 'info', need ? { label: t('smartFix'), run: () => analyzeIds(list.filter((x) => x.score < 100).map((x) => x.id)) } : null);
      loadDuplicates();
    }
  } catch (e) {
    S.scan = null;
    renderAll();
    snack(errMsg(e), 'bad');
  }
}
function openMenu(anchor) {
  const recent = (S.settings.recentFolders || []).slice(0, 6);
  menu(anchor, [
    { icon: 'folderOpen', label: t('openFolder'), end: 'Ctrl+O', run: () => pickFolder(false) },
    ...(!isAndroid() ? [{ icon: 'file', label: t('openFiles'), run: () => pickFiles(false) }] : []),
    ...(S.tracks.length ? [{ icon: 'plus', label: t('addFolder'), run: () => pickFolder(true) }, ...(!isAndroid() ? [{ icon: 'plus', label: t('addFiles'), run: () => pickFiles(true) }] : [])] : []),
    'sep',
    { label: t('recursive'), switch: S.settings.recursive !== false, run: (on) => { S.settings.recursive = on; invoke('save_settings', { settings: S.settings }).then((s) => (S.settings = s)); } },
    ...(S.folder && S.tracks.length && (S.settings.recentFolders || []).includes(S.folder) ? [{ icon: 'refresh', label: t('rescan'), run: () => scanFolder(S.folder) }] : []),
    ...(recent.length ? ['sep', { header: t('recent') }, ...recent.map((f) => ({ icon: 'history', label: baseName(f), run: () => scanFolder(f) }))] : []),
  ]);
}
async function analyzeIds(ids) {
  if (!ids.length) return snack(t('allComplete'));
  try {
    await invoke('analyze_tracks', { ids });
    S.jobs.analyze = { kind: 'analyze', done: 0, total: ids.length, running: true };
    renderJobs();
  } catch (e) { snack(errMsg(e), 'bad'); }
}
async function manualSearch(x) {
  const read = (ov) => ({ title: $('#ms-t', ov).value.trim(), artist: $('#ms-a', ov).value.trim() });
  const r = await dialog({
    title: t('manualSearch'),
    body: `<p style="margin-top:0">${esc(t('manualHint'))}</p><div class="form"><div class="tf full"><label>${esc(t('f_title'))}</label><input class="inp" id="ms-t" value="${esc(x.tags.title || '')}"></div><div class="tf full"><label>${esc(t('f_artist'))}</label><input class="inp" id="ms-a" value="${esc(x.tags.artist || '')}"></div></div>`,
    buttons: [{ label: t('cancel'), value: null }, { label: t('search'), cls: 'filled', icon: 'search', run: read }],
    onMount: (ov, finish) => ov.addEventListener('keydown', (e) => { if (e.key === 'Enter') finish(read(ov)); }),
  });
  if (!r || !r.title) return;
  snack(t('searching'));
  try {
    const p = await invoke('manual_search', { id: x.id, title: r.title, artist: r.artist || null });
    if (p) { S.proposals.set(x.id, p); closeSheet(); go('review', { active: x.id, force: true }); } else snack(t('notFoundMsg'), 'bad');
  } catch (e) { snack(errMsg(e), 'bad'); }
}
async function setCover(ids, dataUrl) {
  await invoke('save_edits', { req: { ids, cover: { op: 'set', dataB64: dataUrl } } });
  snack(t('coverSet', { n: ids.length }));
}
async function coverSearch(ids, query) {
  let results = [], chosen = null;
  const run = async (ov, q) => {
    const grid = $('#cs-grid', ov);
    grid.innerHTML = `<div class="row">${icon('refresh', 'spin')} ${esc(t('searching'))}</div>`;
    try {
      results = await invoke('search_covers', { query: q });
      grid.innerHTML = results.map((c, i) => { coverRes(c.url); return `<div class="cv" data-r="${i}"><span class="check">${icon('check')}</span><div class="img"><img src="${esc(c.thumb)}" alt="" loading="lazy"></div><div class="src ellipsis on-s" title="${esc(c.source)}">${esc(c.source)}</div>${resHTML(c.url)}</div>`; }).join('');
    } catch (e) { grid.innerHTML = `<p>${esc(errMsg(e))}</p>`; }
  };
  const ok = await dialog({
    title: t('coverSearch'), wide: true,
    body: `<div class="search sm" style="max-width:none;margin-bottom:16px">${icon('search')}<input id="cs-q" value="${esc(query)}" placeholder="${esc(t('coverSearchPh'))}"></div><div class="covers" id="cs-grid"></div>`,
    buttons: [{ label: t('cancel'), value: false }, { label: t('useCover'), cls: 'filled', run: () => (chosen === null ? (snack(t('pickCover')), false) : true) }],
    onMount: (ov) => {
      const q = $('#cs-q', ov);
      q.onkeydown = (e) => { if (e.key === 'Enter') run(ov, q.value); };
      $('#cs-grid', ov).onclick = (e) => { const el = e.target.closest('[data-r]'); if (!el) return; chosen = +el.dataset.r; $$('[data-r]', ov).forEach((y) => y.classList.toggle('on', y === el)); };
      if (query) run(ov, query);
    },
  });
  if (!ok || chosen === null) return;
  try {
    const c = results[chosen];
    let b64;
    try { b64 = await invoke('fetch_cover_b64', { url: c.url }); } catch (_) { b64 = await invoke('fetch_cover_b64', { url: c.thumb }); }
    await setCover(ids, b64);
  } catch (e) { snack(errMsg(e), 'bad'); }
}
async function lyricsSearch(x, onPick) {
  let results = [], picked = null;
  const run = async (ov, q) => {
    const box = $('#ls-list', ov);
    box.innerHTML = `<div class="row">${icon('refresh', 'spin')} ${esc(t('searching'))}</div>`;
    try {
      results = await invoke('search_lyrics', { query: q });
      if (!results.length) { box.innerHTML = `<p>${esc(t('notFoundMsg'))}</p>`; return; }
      box.innerHTML = results.map((r, i) => {
        const dd = r.duration ? Math.abs(r.duration - x.duration) : null;
        return `<div class="result" data-i="${i}"><div class="grow"><b>${esc(r.trackName)}</b> — ${esc(r.artistName)}<div class="t-body-s muted">${esc(r.albumName || '')} · ${fmtDur(r.duration)}${dd !== null && dd > 3 ? ` <span class="tag warn">Δ ${Math.round(dd)} s</span>` : ''}</div></div>
          ${r.syncedLyrics ? `<span class="tag ok">${icon('clock')}${esc(t('synced'))}</span>` : r.plainLyrics ? `<span class="tag">${esc(t('plain'))}</span>` : `<span class="tag">${esc(t('instrumentalShort'))}</span>`}</div>`;
      }).join('');
    } catch (e) { box.innerHTML = `<p>${esc(errMsg(e))}</p>`; }
  };
  const q0 = [x.tags.artist, x.tags.title].filter(Boolean).join(' ') || x.fileName.replace(/\.[^.]+$/, '');
  const ok = await dialog({
    title: t('lyricsSearch'), wide: true,
    body: `<div class="search sm" style="max-width:none;margin-bottom:12px">${icon('search')}<input id="ls-q" value="${esc(q0)}"></div><div id="ls-list"></div><div id="ls-prev" class="lyr-prev on-s" style="display:none;margin-top:10px"></div>`,
    buttons: [{ label: t('cancel'), value: false }, { label: t('useLyrics'), cls: 'filled', run: () => (picked ? true : (snack(t('pickResult')), false)) }],
    onMount: (ov) => {
      const q = $('#ls-q', ov);
      q.onkeydown = (e) => { if (e.key === 'Enter') run(ov, q.value); };
      $('#ls-list', ov).onclick = (e) => {
        const el = e.target.closest('[data-i]');
        if (!el) return;
        const r = results[+el.dataset.i];
        picked = r.syncedLyrics || r.plainLyrics;
        $$('.result', ov).forEach((y) => y.classList.toggle('on', y === el));
        const pv = $('#ls-prev', ov);
        pv.style.display = picked ? 'block' : 'none';
        pv.textContent = picked || '';
      };
      run(ov, q0);
    },
  });
  if (ok && picked) onPick(picked);
}
async function batchEdit(ids) {
  const rows = ['artist', 'albumArtist', 'album', 'year', 'genre', 'composer', 'label', 'discTotal', 'trackTotal'];
  const first = S.byId.get(ids[0]);
  const common = (f) => { const v = first?.tags[f]; return ids.every((id) => (S.byId.get(id)?.tags[f] ?? null) === (v ?? null)) ? v ?? '' : ''; };
  let coverData = null, numbering = false;
  const res = await dialog({
    title: t('batchEditN', { n: ids.length }), wide: true,
    body: `<p style="margin-top:0">${esc(t('batchHint'))}</p><div class="form">${rows.map((f) => `<div class="tf"><label><span class="cb" data-on="${f}" style="cursor:pointer">${icon('check')}</span>${esc(fieldLabel(f))}</label><input class="inp" data-bf="${f}" value="${esc(common(f))}" placeholder="${esc(t('mixed'))}"></div>`).join('')}</div>
      <div class="sec-title">${icon('sparkles')}${esc(t('batchTools'))}</div>
      <div class="toggle-row" id="b-num"><button class="switch" tabindex="-1"></button><span class="on-s">${esc(t('autoNumber'))}</span></div>
      <div class="row wrap" style="margin-top:8px"><button class="btn tonal sm has-ic" id="b-cover">${icon('search')}${esc(t('coverSearch'))}</button>${isAndroid() ? '' : `<button class="btn outlined sm has-ic" id="b-cfile">${icon('upload')}${esc(t('coverFile'))}</button>`}<span class="t-body-s" id="b-cinfo"></span></div>`,
    buttons: [{ label: t('cancel'), value: null }, {
      label: t('saveAll'), cls: 'filled', run: (ov) => {
        const fields = {};
        $$('[data-on].on', ov).forEach((cb) => { fields[cb.dataset.on] = $(`[data-bf="${cb.dataset.on}"]`, ov).value.trim() || null; });
        return { fields };
      },
    }],
    onMount: (ov) => {
      $$('[data-on]', ov).forEach((cb) => { cb.onclick = (e) => { e.preventDefault(); cb.classList.toggle('on'); }; });
      $$('[data-bf]', ov).forEach((inp) => { inp.oninput = () => $(`[data-on="${inp.dataset.bf}"]`, ov).classList.add('on'); });
      $('#b-num', ov).onclick = () => { numbering = !numbering; $('#b-num .switch', ov).classList.toggle('on', numbering); };
      $('#b-cover', ov).onclick = () => coverSearch(ids, [common('albumArtist') || common('artist'), common('album')].filter(Boolean).join(' '));
      if ($('#b-cfile', ov)) $('#b-cfile', ov).onclick = async () => {
        const path = await dialogApi()?.open({ multiple: false, filters: [{ name: t('images'), extensions: ['jpg', 'jpeg', 'png', 'webp'] }] });
        if (!path) return;
        try { coverData = await invoke('read_image_file', { path }); $('#b-cinfo', ov).textContent = '✓ ' + baseName(path); } catch (e) { snack(errMsg(e), 'bad'); }
      };
    },
  });
  if (!res) return;
  try {
    if (Object.keys(res.fields).length || coverData)
      await invoke('save_edits', { req: { ids, fields: res.fields, cover: coverData ? { op: 'set', dataB64: coverData } : undefined } });
    if (numbering) {
      const ordered = S.visible.filter((x) => ids.includes(x.id));
      for (let i = 0; i < ordered.length; i++) await invoke('save_edits', { req: { ids: [ordered[i].id], fields: { track: String(i + 1), trackTotal: String(ordered.length) } } });
    }
    snack(t('savedN', { n: ids.length }));
  } catch (e) { snack(errMsg(e), 'bad'); }
}
async function renameDialog(ids) {
  const tokens = ['%artist%', '%title%', '%album%', '%track%', '%year%', '%albumartist%', '%disc%', '%genre%'];
  const presets = ['%artist% - %title%', '%track% - %title%', '%track%. %artist% - %title%', '%artist% - %album% - %track% - %title%'];
  const preview = async (ov) => {
    const list = await invoke('rename_preview', { ids, pattern: $('#rn-p', ov).value });
    const changed = list.filter((r) => r.new && r.new !== r.old);
    $('#rn-sum', ov).textContent = t('renameSum', { n: changed.length, c: list.filter((r) => r.conflict).length });
    $('#rn-tbl', ov).innerHTML = `<table class="table"><tr><th>${esc(t('before'))}</th><th>${esc(t('after'))}</th></tr>${list.slice(0, 300).map((r) => `<tr><td class="muted">${esc(r.old)}</td><td class="${r.conflict ? 'bad' : ''}">${esc(r.new || t('noTitleSkip'))}</td></tr>`).join('')}</table>`;
  };
  const ok = await dialog({
    title: t('renameN', { n: ids.length }), wide: true,
    body: `<div class="tf"><label>${esc(t('pattern'))}</label><input class="inp mono" id="rn-p" value="${esc(S.settings.renamePattern || presets[0])}"></div>
      <div class="chips" style="margin:10px 0;flex-wrap:wrap">${tokens.map((tk) => `<button class="chip" data-tk="${tk}">${tk}</button>`).join('')}</div>
      <div class="row wrap" style="margin-bottom:10px">${presets.map((p) => `<button class="btn text sm" data-pr="${esc(p)}">${esc(p)}</button>`).join('')}</div>
      <div class="on-s t-title-s" id="rn-sum" style="margin-bottom:6px"></div><div id="rn-tbl" style="max-height:40vh;overflow:auto"></div>`,
    buttons: [{ label: t('cancel'), value: false }, { label: t('renameDo'), cls: 'filled', icon: 'rename', run: (ov) => $('#rn-p', ov).value }],
    onMount: (ov) => {
      const p = $('#rn-p', ov);
      const pv = debounce(() => preview(ov), 200);
      p.oninput = pv;
      $$('[data-tk]', ov).forEach((b) => { b.onclick = () => { const s = p.selectionStart ?? p.value.length; p.value = p.value.slice(0, s) + b.dataset.tk + p.value.slice(p.selectionEnd ?? s); p.focus(); pv(); }; });
      $$('[data-pr]', ov).forEach((b) => { b.onclick = () => { p.value = b.dataset.pr; pv(); }; });
      preview(ov);
    },
  });
  if (!ok) return;
  try {
    const n = await invoke('rename_apply', { ids, pattern: ok });
    S.settings.renamePattern = ok;
    invoke('save_settings', { settings: S.settings });
    snack(t('renamed', { n }));
  } catch (e) { snack(errMsg(e), 'bad'); }
}
async function folderBrowser() {
  let cur = null;
  const load = async (ov, path) => {
    const list = $('#fb-list', ov), crumbs = $('#fb-crumbs', ov);
    try {
      if (!path) {
        const roots = await invoke('get_roots');
        cur = null;
        crumbs.innerHTML = `<span>${esc(t('locations'))}</span>`;
        list.innerHTML = roots.map((r) => `<div class="dir" data-p="${esc(r.path)}">${icon('home')}<span class="grow">${esc(r.name)}</span><span class="t-body-s muted ellipsis" style="max-width:50%">${esc(r.path)}</span></div>`).join('') || `<p>${esc(t('noLocations'))}</p>`;
        $('#fb-info', ov).textContent = '';
        return;
      }
      const d = await invoke('list_dirs', { path });
      cur = d.path;
      const pts = d.path.split(/[\\/]/).filter(Boolean);
      const win = /^[A-Za-z]:/.test(d.path);
      crumbs.innerHTML = `<span data-p="">${icon('home', 'sm')}</span>` + pts.map((p, i) => {
        const full = (win ? '' : '/') + pts.slice(0, i + 1).join(win ? '\\' : '/') + (win && i === 0 ? '\\' : '');
        return `›<span data-p="${esc(full)}">${esc(p)}</span>`;
      }).join('');
      list.innerHTML = (d.parent ? `<div class="dir" data-p="${esc(d.parent)}">${icon('arrowUp')}<span>..</span></div>` : '') +
        d.dirs.map((x) => `<div class="dir" data-p="${esc(x.path)}">${icon('folder')}<span class="grow ellipsis">${esc(x.name)}</span>${icon('chevronRight', 'dim')}</div>`).join('');
      $('#fb-info', ov).textContent = t('audioHere', { n: d.audioHere });
    } catch (e) { snack(errMsg(e), 'bad'); }
  };
  return dialog({
    title: t('openFolder'),
    body: `<div class="crumbs on-s" id="fb-crumbs"></div><div class="dir-list" id="fb-list"></div>
      <div class="toggle-row" id="fb-rec" style="margin-top:12px"><button class="switch ${S.settings.recursive !== false ? 'on' : ''}" tabindex="-1"></button><span class="on-s">${esc(t('recursive'))}</span></div>
      <div class="t-body-s" id="fb-info" style="margin-top:6px"></div>`,
    buttons: [{ label: t('cancel'), value: null }, { label: t('useThisFolder'), cls: 'filled', icon: 'check', run: () => cur || (snack(t('pickFolderFirst')), false) }],
    onMount: (ov) => {
      ov.addEventListener('click', (e) => { const el = e.target.closest('[data-p]'); if (el && ov.contains(el)) load(ov, el.dataset.p || null); });
      $('#fb-rec', ov).onclick = () => { S.settings.recursive = S.settings.recursive === false; $('#fb-rec .switch', ov).classList.toggle('on', S.settings.recursive); };
      load(ov, null);
    },
  });
}
async function advancedFilters() {
  const formats = [...new Set(S.tracks.map((x) => x.format))].sort();
  const fc = new Map();
  for (const x of S.tracks) fc.set(x.folder, (fc.get(x.folder) || 0) + 1);
  const folders = [...fc.keys()].sort();
  const a = { formats: new Set(S.adv.formats), folders: new Set(S.adv.folders), statuses: new Set(S.adv.statuses), missing: new Set(S.adv.missing), min: S.adv.min, max: S.adv.max };
  const chips = (k, list, label) => list.map((v) => `<button class="chip ${a[k].has(v) ? 'on' : ''}" data-fk="${k}" data-fv="${esc(v)}">${icon('check', 'chk')}${esc(label(v))}</button>`).join('');
  const ok = await dialog({
    title: t('filters'), wide: true,
    body: `<div class="flt-sec"><h4>${esc(t('fltMissing'))}</h4><div class="chips">${chips('missing', MISS_KEYS, (v) => t('mi_' + v))}</div></div>
      <div class="flt-sec"><h4>${esc(t('fltStatus'))}</h4><div class="chips">${chips('statuses', STATUSES, (v) => t('st_' + v))}</div></div>
      <div class="flt-sec"><h4>${esc(t('fltFormat'))}</h4><div class="chips">${chips('formats', formats, (v) => v)}</div></div>
      ${folders.length > 1 ? `<div class="flt-sec"><h4>${esc(t('fltFolder'))}</h4><div class="chips">${chips('folders', folders, (v) => `${baseName(v)} (${fc.get(v)})`)}</div></div>` : ''}
      <div class="flt-sec"><h4>${esc(t('fltScore'))}</h4><div class="row on-s wrap"><span>${esc(t('from'))}</span><input type="range" id="f-min" min="0" max="100" step="5" value="${a.min}" class="grow"><b id="f-minv" style="width:44px">${a.min}%</b>
        <span>${esc(t('to'))}</span><input type="range" id="f-max" min="0" max="100" step="5" value="${a.max}" class="grow"><b id="f-maxv" style="width:44px">${a.max}%</b></div></div>`,
    buttons: [{ label: t('clearFilters'), value: 'clear' }, { label: t('cancel'), value: false }, { label: t('applyFilters'), cls: 'filled', value: true }],
    onMount: (ov) => {
      $$('[data-fk]', ov).forEach((b) => { b.onclick = () => { const s = a[b.dataset.fk], v = b.dataset.fv; s.has(v) ? s.delete(v) : s.add(v); b.classList.toggle('on', s.has(v)); }; });
      $('#f-min', ov).oninput = (e) => { a.min = Math.min(+e.target.value, a.max); e.target.value = a.min; $('#f-minv', ov).textContent = a.min + '%'; };
      $('#f-max', ov).oninput = (e) => { a.max = Math.max(+e.target.value, a.min); e.target.value = a.max; $('#f-maxv', ov).textContent = a.max + '%'; };
    },
  });
  if (ok === 'clear') clearFilters();
  else if (ok === true) { S.adv = a; refreshLibrary(true); }
}
function clearFilters() {
  S.adv = { formats: new Set(), folders: new Set(), statuses: new Set(), missing: new Set(), min: 0, max: 100 };
  S.quick = 'all';
  S.album = null;
  S.query = '';
  const q = $('#q');
  if (q) q.value = '';
  refreshLibrary(true);
  renderTopbar();
}
async function exportReport() {
  const head = ['path', 'artist', 'title', 'album', 'albumArtist', 'year', 'track', 'genre', 'cover', 'lyrics', 'synced', 'score', 'missing'];
  const q = (v) => `"${String(v ?? '').replace(/"/g, '""')}"`;
  const rows = S.visible.map((x) => [x.path, x.tags.artist, x.tags.title, x.tags.album, x.tags.albumArtist, x.tags.year, x.tags.track, x.tags.genre,
    x.cover ? `${x.cover.width}x${x.cover.height}` : '', x.lyrics ? 'yes' : '', x.lyricsSynced ? 'yes' : '', x.score, x.missing.join(' ')].map(q).join(';'));
  let path = '';
  const d = dialogApi();
  if (d && !isAndroid()) {
    path = await d.save({ defaultPath: 'musictagger-raport.csv', filters: [{ name: 'CSV', extensions: ['csv'] }] });
    if (!path) return;
  }
  try { const p = await invoke('write_text_file', { path, content: [head.join(';'), ...rows].join('\r\n') }); snack(t('reportSaved', { p })); } catch (e) { snack(errMsg(e), 'bad'); }
}
async function loadDuplicates() {
  try {
    S.dupGroups = await invoke('find_duplicates');
    S.dupIndex = new Map();
    S.dupGroups.forEach((g, i) => g.forEach((id) => S.dupIndex.set(id, i)));
  } catch (_) { S.dupGroups = []; S.dupIndex = new Map(); }
  if (S.view === 'library' && S.tracks.length && $('#tools')) renderTools();
}
function libMore(anchor) {
  menu(anchor, [
    { icon: 'checkAll', label: t('selectAllVisible'), end: 'Ctrl+A', run: () => { S.visible.forEach((x) => S.selected.add(x.id)); refreshLibrary(); renderTopbar(); } },
    { icon: 'filter', label: t('selectIncomplete'), run: () => { S.visible.filter((x) => x.score < 100).forEach((x) => S.selected.add(x.id)); refreshLibrary(); renderTopbar(); } },
    'sep',
    { icon: 'report', label: t('exportReport'), run: exportReport },
    { icon: 'undo', label: t('undoLast'), end: 'Ctrl+Z', run: undoLast },
  ]);
}
function sortMenu(anchor) {
  const keys = ['path', 'title', 'artist', 'album', 'score', 'duration', 'modified'];
  menu(anchor, [
    ...keys.map((k) => ({ icon: S.sort.key === k ? 'check' : '', label: t('sort_' + k), run: () => { S.sort = { key: k, dir: S.sort.key === k ? -S.sort.dir : 1 }; refreshLibrary(true); } })),
    'sep',
    { icon: 'sort', label: S.sort.dir > 0 ? t('ascending') : t('descending'), run: () => { S.sort.dir = -S.sort.dir; refreshLibrary(true); } },
  ]);
}
function selMore(anchor) {
  const ids = [...S.selected];
  const withProp = ids.filter((id) => S.proposals.has(id) && S.byId.get(id)?.status === 'proposed');
  menu(anchor, [
    { icon: 'checkAll', label: t('selectAllVisible'), run: () => { S.visible.forEach((x) => S.selected.add(x.id)); refreshLibrary(); renderTopbar(); } },
    { icon: 'x', label: t('invertSel'), run: () => { S.visible.forEach((x) => (S.selected.has(x.id) ? S.selected.delete(x.id) : S.selected.add(x.id))); refreshLibrary(); renderTopbar(); } },
    'sep',
    { icon: 'image', label: t('coverForSel'), run: () => coverSearch(ids, '') },
    ...(withProp.length ? [{ icon: 'sparkles', label: t('reviewSel', { n: withProp.length }), run: () => go('review', { active: withProp[0] }) }, { icon: 'x', label: t('rejectN', { n: withProp.length }), run: () => rejectIds(withProp) }] : []),
  ]);
}

// ═════════════════════════════ Zdarzenia ═════════════════════════════
document.addEventListener('click', async (e) => {
  const el = e.target.closest('[data-nav],[data-open],[data-quick],[data-mode],[data-album],[data-tab],[data-sort],[data-rm],[data-act],[data-rv],[data-rvsel],[data-rvconf],[data-rvtype],[data-undo],[data-theme-toggle]');
  if (!el || el.closest('.menu')) return;
  const d = el.dataset;
  if (d.nav) return go(d.nav);
  if (d.open) return scanFolder(d.open);
  if (d.quick) { S.quick = S.quick === d.quick && d.quick !== 'all' ? 'all' : d.quick; if (S.mode === 'albums') S.mode = 'list'; return refreshLibrary(true); }
  if (d.mode) { S.mode = d.mode; store('mode', S.mode); return refreshLibrary(true); }
  if (d.album) { S.album = d.album; return refreshLibrary(true); }
  if (d.tab) { S.tab = d.tab; stopAudio(); return renderSheet(); }
  if (d.sort) { S.sort = { key: d.sort, dir: S.sort.key === d.sort ? -S.sort.dir : 1 }; return refreshLibrary(true); }
  if (d.rm) {
    if (d.rm === 'album') S.album = null;
    else if (d.rm === 'query') { S.query = ''; renderTopbar(); }
    else if (d.rm === 'range') { S.adv.min = 0; S.adv.max = 100; }
    else S.adv[d.rm].delete(d.v);
    return refreshLibrary(true);
  }
  if (d.rvsel) { e.stopPropagation(); S.rv.sel.has(d.rvsel) ? S.rv.sel.delete(d.rvsel) : S.rv.sel.add(d.rvsel); return refreshReview(); }
  if (d.rv) { S.rv.active = d.rv; refreshReview(); openReviewDetail(); return; }
  if (d.rvconf) { S.rv.conf = d.rvconf; return refreshReview(); }
  if (d.rvtype) { S.rv.types.has(d.rvtype) ? S.rv.types.delete(d.rvtype) : S.rv.types.add(d.rvtype); return refreshReview(); }
  if (d.undo) return undoIds([d.undo]);
  if (el.hasAttribute('data-theme-toggle')) { S.settings.theme = S.settings.theme === 'light' ? 'dark' : 'light'; applyTheme(); renderNav(); invoke('save_settings', { settings: S.settings }); return; }
  const x = S.active && S.byId.get(S.active);
  switch (d.act) {
    case 'open-menu': return openMenu(el);
    case 'open-folder': return pickFolder(false);
    case 'open-files': return pickFiles(false);
    case 'toggle-recursive': S.settings.recursive = S.settings.recursive === false; S.settings = await invoke('save_settings', { settings: S.settings }); return renderView();
    case 'rescan-recursive': return scanFolder(S.folder, { recursive: true });
    case 'lib-more': return libMore(el);
    case 'sort-menu': return sortMenu(el);
    case 'adv-open': return advancedFilters();
    case 'clear-filters': return clearFilters();
    case 'fab': return analyzeIds(S.visible.filter((y) => y.score < 100 && !['proposed', 'queued', 'working'].includes(y.status)).map((y) => y.id));
    case 'search-m': { const r = await promptBox(t('search'), t('searchPh'), S.query); if (r !== null) { S.query = r; refreshLibrary(true); renderTopbar(); } return; }
    case 'sel-all': { const all = S.visible.every((y) => S.selected.has(y.id)); S.visible.forEach((y) => (all ? S.selected.delete(y.id) : S.selected.add(y.id))); paintRows(); renderTopbar(); $('.fab')?.classList.toggle('hide', S.selected.size > 0); return; }
    case 'sel-clear': S.selected.clear(); paintRows(); renderTopbar(); $('.fab')?.classList.remove('hide'); return;
    case 'sel-analyze': return analyzeIds([...S.selected]);
    case 'sel-accept': return acceptIds([...S.selected].filter((id) => S.byId.get(id)?.status === 'proposed'));
    case 'sel-edit': return batchEdit([...S.selected]);
    case 'sel-rename': return renameDialog([...S.selected]);
    case 'sel-more': return selMore(el);
    case 'cancel': return invoke('cancel_job');
    case 'perm-check': S.storageOk = await invoke('storage_access'); return renderView();
    case 'analyze-all': go('library'); return analyzeIds(S.tracks.filter((y) => y.score < 100).map((y) => y.id));
    case 'sheet-close': return closeSheet();
    case 'sheet-prev': return stepSheet(-1);
    case 'sheet-next': return stepSheet(1);
    case 'sheet-compare': { const id = S.active; closeSheet(); return go('review', { active: id, force: true }); }
    case 'analyze-one': return x && analyzeIds([x.id]);
    case 'manual-search': return x && manualSearch(x);
    case 'cover-search': return x && coverSearch([x.id], [x.tags.albumArtist || x.tags.artist, x.tags.album || x.tags.title].filter(Boolean).join(' '));
    case 'cover-url': {
      const url = await promptBox(t('coverUrl'), 'URL', '', 'https://…');
      if (!url || !x) return;
      try { await setCover([x.id], await invoke('fetch_cover_b64', { url })); } catch (err) { snack(errMsg(err), 'bad'); }
      return;
    }
    case 'cover-file': {
      const path = await dialogApi()?.open({ multiple: false, filters: [{ name: t('images'), extensions: ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'gif'] }] });
      if (!path || !x) return;
      try { await setCover([x.id], await invoke('read_image_file', { path })); } catch (err) { snack(errMsg(err), 'bad'); }
      return;
    }
    case 'cover-export': {
      const dest = await dialogApi()?.save({ defaultPath: (x.tags.album || 'cover') + '.jpg', filters: [{ name: 'JPEG', extensions: ['jpg'] }] });
      if (!dest) return;
      try { await invoke('export_cover', { id: x.id, dest }); snack(t('exported')); } catch (err) { snack(errMsg(err), 'bad'); }
      return;
    }
    case 'cover-remove':
      if (!(await confirmBox(t('coverRemove'), t('coverRemoveQ'), t('remove'), true))) return;
      try { await invoke('save_edits', { req: { ids: [x.id], cover: { op: 'remove' } } }); snack(t('saved')); } catch (err) { snack(errMsg(err), 'bad'); }
      return;
    case 'lyrics-search': return x && lyricsSearch(x, (txt) => { const ta = $('#l-text'); if (ta) { ta.value = txt; $('#l-save').disabled = false; } });
    case 'reveal': return opener()?.revealItemInDir(x.path).catch((err) => snack(errMsg(err), 'bad'));
    case 'open-mb': return opener()?.openUrl(`https://musicbrainz.org/recording/${x.tags.mbRecordingId}`);
    case 'reload-track': { const nt = await invoke('reload_track', { id: x.id }); upsert(nt); renderSheet(); paintRows(); return; }
    case 'rv-accept': return reviewDecide('accept');
    case 'rv-reject': return reviewDecide('reject');
    case 'rv-skip': return reviewDecide('skip');
    case 'rv-prev': return reviewStep(-1);
    case 'rv-next': return reviewStep(1);
    case 'rv-back': return closeReviewDetail();
    case 'rv-edit': return S.rv.active && openSheet(S.rv.active, 'tags');
    case 'rv-accept-sure': {
      const th = S.settings.autoApplyThreshold ?? 0.9;
      const ids = pendingProposals().filter((y) => S.proposals.get(y.id).confidence >= th).map((y) => y.id);
      if (ids.length && (await confirmBox(t('acceptSureQ'), t('acceptSureText', { n: ids.length, p: Math.round(th * 100) }), t('accept')))) acceptIds(ids);
      return;
    }
    case 'rv-more': return reviewMore(el);
    case 'rv-sel-accept': return acceptIds([...S.rv.sel]);
    case 'rv-sel-reject': return rejectIds([...S.rv.sel]);
    case 'rv-sel-clear': S.rv.sel.clear(); return refreshReview();
    case 'h-clear':
      if (await confirmBox(t('clearHistory'), t('clearHistoryQ'), t('remove'), true)) { await invoke('clear_history'); loadHistory(); }
      return;
  }
});
const onSearch = debounce(() => refreshLibrary(true), 120);
const onRvSearch = debounce(() => refreshReview(), 150);
document.addEventListener('input', (e) => {
  if (e.target.id === 'q') { S.query = e.target.value; onSearch(); }
  if (e.target.id === 'rv-q') { S.rv.q = e.target.value; onRvSearch(); }
});
document.addEventListener('keydown', (e) => {
  const inField = e.target.matches('input, textarea, [contenteditable="true"], select');
  const mod = e.ctrlKey || e.metaKey;
  if (e.key === 'Escape') {
    if (closeTop()) return;
    if (S.selected.size && S.view === 'library') { S.selected.clear(); paintRows(); renderTopbar(); $('.fab')?.classList.remove('hide'); }
    return;
  }
  if (mod && e.key.toLowerCase() === 'f') { e.preventDefault(); if (S.view === 'review') return $('#rv-q')?.focus(); if (S.view !== 'library') go('library'); setTimeout(() => $('#q')?.focus(), 0); return; }
  if (mod && e.key.toLowerCase() === 'o') { e.preventDefault(); return pickFolder(false); }
  if (inField || $('#layers').children.length || $('.menu')) return;
  if (mod && e.key.toLowerCase() === 'a' && S.view === 'library') { e.preventDefault(); S.visible.forEach((x) => S.selected.add(x.id)); paintRows(); renderTopbar(); $('.fab')?.classList.add('hide'); return; }
  if (mod && e.key.toLowerCase() === 'z') { e.preventDefault(); return undoLast(); }
  if (S.active) {
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') { e.preventDefault(); return stepSheet(e.key === 'ArrowDown' ? 1 : -1); }
    if (e.key === ' ' && S.tab === 'lyrics') { e.preventDefault(); return $('#l-play')?.click(); }
    return;
  }
  if (S.view === 'review' && !mod) {
    const k = e.key.toLowerCase();
    if (k === 'a' || e.key === 'Enter') { e.preventDefault(); return reviewDecide('accept'); }
    if (k === 'd' || e.key === 'Delete') { e.preventDefault(); return reviewDecide('reject'); }
    if (k === 's') { e.preventDefault(); return reviewDecide('skip'); }
    if (e.key === 'ArrowRight' || e.key === 'ArrowDown') { e.preventDefault(); return reviewStep(1); }
    if (e.key === 'ArrowLeft' || e.key === 'ArrowUp') { e.preventDefault(); return reviewStep(-1); }
    if (k === 'e') return S.rv.active && openSheet(S.rv.active, 'tags');
  }
});
let lastMobile = isMobile();
window.addEventListener('resize', debounce(() => {
  if (isMobile() !== lastMobile) { lastMobile = isMobile(); return renderAll(); }
  if (S.view === 'library') paintRows();
}, 120));
matchMedia('(prefers-color-scheme: light)').addEventListener?.('change', applyTheme);

// ═════════════════════════════ Start ═════════════════════════════
async function init() {
  try {
    S.settings = await invoke('get_settings');
    S.info = await invoke('app_info');
  } catch (e) { console.error(e); }
  setLang(S.settings.language || 'auto');
  applyTheme();
  if (isAndroid()) S.storageOk = await invoke('storage_access').catch(() => true);
  renderAll();

  listen('scan-progress', (e) => { S.scan = e.payload; renderJobs(); });
  listen('track-update', (e) => {
    const { track, proposal } = e.payload;
    upsert(track, proposal);
    schedule('rows');
    if (S.view === 'review') schedule('review');
    if (S.active === track.id) schedule('sheet');
  });
  listen('track-replaced', (e) => {
    replaceTrack(e.payload.oldId, e.payload.track);
    schedule('rows');
    if (S.active === e.payload.track.id) schedule('sheet');
  });
  listen('job-progress', (e) => {
    const j = e.payload;
    S.jobs[j.kind] = j;
    renderJobs();
    renderTopbar();
    if (j.running) return;
    if (j.kind === 'analyze') {
      const proposed = pendingProposals().length;
      const applied = S.tracks.filter((x) => x.status === 'applied').length;
      const nf = S.tracks.filter((x) => x.status === 'notFound' || x.status === 'error').length;
      snack(t('analyzeDone', { p: proposed, a: applied, nf }), 'info', proposed && S.view !== 'review' ? { label: t('verify'), run: () => go('review') } : null);
    } else if (j.total) {
      snack(t('applyDone', { n: j.done - j.errors, e: j.errors }), j.errors ? 'bad' : 'info', { label: t('undo'), run: undoLast });
    }
    schedule(S.view === 'review' ? 'review' : 'rows');
    loadDuplicates();
  });
  listen('tauri://drag-enter', () => { const d = $('#drop'); d.hidden = false; d.textContent = t('dropHere'); });
  listen('tauri://drag-leave', () => { $('#drop').hidden = true; });
  listen('tauri://drag-drop', (e) => {
    $('#drop').hidden = true;
    const paths = e.payload?.paths || [];
    if (!paths.length) return;
    const files = paths.filter((p) => /\.[a-z0-9]{2,4}$/i.test(p));
    if (files.length === paths.length) runScan(t('nFiles', { n: files.length }), () => invoke('scan_files', { paths: files, append: S.tracks.length > 0 }), S.tracks.length > 0);
    else scanFolder(paths.find((p) => !files.includes(p)));
  });
  const last = store('updChecked');
  if (!last || Date.now() - +last > 86400000) { store('updChecked', String(Date.now())); setTimeout(() => checkUpdate(true), 4000); }
}
init();
