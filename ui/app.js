import { icon, LOGO } from './icons.js';
import { t, setLang, currentLang } from './i18n.js';

// ───────────────────────────── Most do backendu ─────────────────────────────
const TAURI = window.__TAURI__;
const invoke = (cmd, args) => (TAURI ? TAURI.core.invoke(cmd, args) : window.__MOCK__.invoke(cmd, args));
const listen = (ev, cb) => (TAURI ? TAURI.event.listen(ev, cb) : window.__MOCK__?.listen?.(ev, cb));
const fileSrc = (p) => (TAURI ? TAURI.core.convertFileSrc(p) : '');
const dialog = TAURI?.dialog;
const opener = TAURI?.opener;

// ───────────────────────────── Stan ─────────────────────────────
const S = {
  settings: null,
  info: { version: '', platform: '' },
  tracks: [],
  byId: new Map(),
  proposals: new Map(),
  folder: null,
  view: 'library',
  filter: 'all',
  query: '',
  sort: { key: 'path', dir: 1 },
  mode: localStore('mode') || 'list',
  album: null,
  selected: new Set(),
  anchor: null,
  active: null,
  tab: 'edit',
  job: null,
  scan: null,
  covers: new Map(),
  dups: null,
  history: null,
  visible: [],
  storageOk: true,
  reviewSel: new Set(),
};
const isMobile = () => window.matchMedia('(max-width: 760px)').matches;
const isAndroid = () => S.info.platform === 'android';

function localStore(k, v) {
  try {
    if (v === undefined) return localStorage.getItem('mt.' + k);
    localStorage.setItem('mt.' + k, v);
  } catch (_) { return null; }
}

// ───────────────────────────── Pomocnicze ─────────────────────────────
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => [...r.querySelectorAll(s)];
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
const fmtDur = (s) => {
  if (!s || s < 0) return '0:00';
  s = Math.round(s);
  const h = Math.floor(s / 3600), m = Math.floor((s % 3600) / 60), sec = s % 60;
  return (h ? h + ':' + String(m).padStart(2, '0') : m) + ':' + String(sec).padStart(2, '0');
};
const fmtSize = (b) => (b > 1 << 30 ? (b / (1 << 30)).toFixed(2) + ' GB' : b > 1 << 20 ? (b / (1 << 20)).toFixed(1) + ' MB' : Math.round(b / 1024) + ' KB');
const scoreColor = (s) => (s >= 85 ? 'var(--ok)' : s >= 60 ? 'var(--warn)' : 'var(--bad)');
const debounce = (fn, ms) => { let h; return (...a) => { clearTimeout(h); h = setTimeout(() => fn(...a), ms); }; };
const errMsg = (e) => (typeof e === 'string' ? e : e?.message || JSON.stringify(e));
const baseName = (p) => String(p || '').split(/[\\/]/).filter(Boolean).pop() || p;
const fieldLabel = (f) => t('f_' + f);
const TAG_FIELDS = ['title', 'artist', 'album', 'albumArtist', 'year', 'track', 'trackTotal', 'disc', 'discTotal', 'genre', 'composer', 'label', 'isrc', 'bpm', 'comment'];
const CORE = ['title', 'artist', 'album', 'year', 'track'];

let renderQueued = false;
function scheduleRender(what = 'all') {
  scheduleRender.parts = scheduleRender.parts || new Set();
  scheduleRender.parts.add(what);
  if (renderQueued) return;
  renderQueued = true;
  requestAnimationFrame(() => {
    renderQueued = false;
    const parts = scheduleRender.parts;
    scheduleRender.parts = new Set();
    if (parts.has('all')) return renderAll();
    if (parts.has('view')) renderView();
    if (parts.has('rows')) { computeVisible(); paintRows(); updateCounts(); }
    if (parts.has('drawer')) renderDrawer();
    renderSelbar();
    renderNavBadges();
  });
}

// ───────────────────────────── Toasty i modale ─────────────────────────────
function toast(msg, kind = 'info', action) {
  const el = document.createElement('div');
  el.className = 'toast ' + kind;
  el.innerHTML = icon(kind === 'ok' ? 'check' : kind === 'bad' ? 'alert' : 'info') + `<div class="grow selectable">${esc(msg)}</div>`;
  if (action) {
    const b = document.createElement('button');
    b.className = 'btn sm';
    b.textContent = action.label;
    b.onclick = () => { action.run(); close(); };
    el.appendChild(b);
  }
  const close = () => { el.classList.add('out'); setTimeout(() => el.remove(), 260); };
  el.addEventListener('click', (e) => { if (e.target === el) close(); });
  const box = $('#toasts');
  const max = isMobile() ? 1 : 3;
  while (box.children.length >= max) box.firstElementChild.remove();
  box.appendChild(el);
  setTimeout(close, kind === 'bad' ? 8000 : action ? 9000 : 4200);
}

const modalStack = [];
function modal({ title, body, buttons = [], wide = false, onMount }) {
  return new Promise((resolve) => {
    const ov = document.createElement('div');
    ov.className = 'overlay';
    ov.innerHTML = `<div class="modal ${wide ? 'wide' : ''}" role="dialog" aria-modal="true">
      <div class="modal-head"><h3>${esc(title)}</h3><button class="btn icon ghost" data-x>${icon('x')}</button></div>
      <div class="modal-body">${body}</div>
      ${buttons.length ? `<div class="modal-foot">${buttons.map((b, i) => `<button class="btn ${b.cls || ''}" data-b="${i}">${b.icon ? icon(b.icon) : ''}<span>${esc(b.label)}</span></button>`).join('')}</div>` : ''}
    </div>`;
    const done = (v) => {
      ov.remove();
      const i = modalStack.indexOf(entry);
      if (i >= 0) modalStack.splice(i, 1);
      resolve(v);
    };
    const entry = { close: () => done(null) };
    modalStack.push(entry);
    pushBack();
    ov.addEventListener('mousedown', (e) => { if (e.target === ov) done(null); });
    $('[data-x]', ov).onclick = () => done(null);
    $$('[data-b]', ov).forEach((btn) => {
      btn.onclick = async () => {
        const b = buttons[+btn.dataset.b];
        if (b.run) {
          btn.disabled = true;
          try {
            const r = await b.run(ov);
            if (r !== false) done(r ?? b.value ?? true);
          } catch (e) { toast(errMsg(e), 'bad'); }
          btn.disabled = false;
        } else done(b.value ?? null);
      };
    });
    $('#modal-root').appendChild(ov);
    onMount?.(ov, done);
    setTimeout(() => $('input:not([type=checkbox]),textarea', ov)?.focus(), 30);
  });
}
const confirmBox = (title, text, okLabel = t('ok'), danger = false) =>
  modal({ title, body: `<p class="muted" style="margin:0;line-height:1.5">${esc(text)}</p>`, buttons: [{ label: t('cancel'), value: false }, { label: okLabel, cls: danger ? 'primary danger' : 'primary', value: true }] });
const promptBox = (title, label, value = '', placeholder = '') =>
  modal({
    title,
    body: `<div class="field"><label>${esc(label)}</label><input class="inp" id="pb" value="${esc(value)}" placeholder="${esc(placeholder)}"></div>`,
    buttons: [{ label: t('cancel'), value: null }, { label: t('ok'), cls: 'primary', run: (ov) => $('#pb', ov).value }],
    onMount: (ov, done) => { $('#pb', ov).onkeydown = (e) => { if (e.key === 'Enter') done($('#pb', ov).value); }; },
  });

// Przycisk „wstecz” Androida zamyka modale/panel.
let backDepth = 0;
function pushBack() { backDepth++; history.pushState({ d: backDepth }, ''); }
window.addEventListener('popstate', () => {
  backDepth = Math.max(0, backDepth - 1);
  if (modalStack.length) return modalStack[modalStack.length - 1].close();
  if (S.active) return closeDrawer(true);
  if (S.selected.size) { S.selected.clear(); return scheduleRender('rows'); }
  if (S.view !== 'library') return go('library');
});

// ───────────────────────────── Dane ─────────────────────────────
function setTracks(list) {
  S.tracks = list;
  S.byId = new Map(list.map((x) => [x.id, x]));
  S.proposals.clear();
  S.selected.clear();
  S.covers.clear();
  S.dups = null;
  S.album = null;
}
function upsert(tr, proposal) {
  const old = S.byId.get(tr.id);
  if (old) {
    const coverChanged = JSON.stringify(old.cover) !== JSON.stringify(tr.cover) || old.path !== tr.path;
    Object.assign(old, tr);
    if (coverChanged) S.covers.delete(tr.id);
  } else {
    S.tracks.push(tr);
    S.byId.set(tr.id, tr);
  }
  if (proposal) S.proposals.set(tr.id, proposal);
  else if (!['proposed', 'error'].includes(tr.status)) S.proposals.delete(tr.id);
}
function replaceTrack(oldId, tr) {
  const i = S.tracks.findIndex((x) => x.id === oldId);
  if (i >= 0) S.tracks[i] = tr; else S.tracks.push(tr);
  S.byId.delete(oldId);
  S.byId.set(tr.id, tr);
  const p = S.proposals.get(oldId);
  S.proposals.delete(oldId);
  if (p) { p.trackId = tr.id; S.proposals.set(tr.id, p); }
  S.covers.delete(oldId);
  if (S.selected.delete(oldId)) S.selected.add(tr.id);
  if (S.active === oldId) S.active = tr.id;
}

const FILTERS = {
  all: () => true,
  missingTags: (x) => CORE.some((f) => x.missing.includes(f)) || x.missing.includes('genre'),
  noCover: (x) => x.missing.includes('cover') || x.missing.includes('coverSmall'),
  noLyrics: (x) => x.missing.includes('lyrics'),
  noSynced: (x) => !x.lyricsSynced,
  proposed: (x) => S.proposals.has(x.id) && x.status === 'proposed',
  complete: (x) => x.score >= 90,
  problems: (x) => x.status === 'notFound' || x.status === 'error',
};

function matchesQuery(x, q) {
  if (!q) return true;
  const hay = [x.tags.title, x.tags.artist, x.tags.album, x.tags.albumArtist, x.tags.genre, x.tags.year, x.fileName].join(' ').toLowerCase()
    .normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/ł/g, 'l');
  return q.split(/\s+/).every((w) => hay.includes(w));
}
const albumKey = (x) => (x.tags.album ? (x.tags.albumArtist || x.tags.artist || '') + '\u0001' + x.tags.album : '\u0002' + x.folder);

function computeVisible() {
  const q = S.query.trim().toLowerCase().normalize('NFD').replace(/[̀-ͯ]/g, '').replace(/ł/g, 'l');
  const f = FILTERS[S.filter] || FILTERS.all;
  let list = S.tracks.filter((x) => f(x) && matchesQuery(x, q) && (!S.album || albumKey(x) === S.album));
  const { key, dir } = S.sort;
  if (key !== 'path') {
    const get = {
      title: (x) => (x.tags.title || x.fileName).toLowerCase(),
      artist: (x) => (x.tags.artist || '~').toLowerCase(),
      album: (x) => (x.tags.album || '~').toLowerCase() + String(x.tags.disc || 1).padStart(2, '0') + String(x.tags.track || 999).padStart(3, '0'),
      score: (x) => x.score,
      modified: (x) => x.modified,
      duration: (x) => x.duration,
    }[key];
    list = list.slice().sort((a, b) => { const A = get(a), B = get(b); return (A < B ? -1 : A > B ? 1 : 0) * dir; });
  } else if (dir < 0) list = list.slice().reverse();
  S.visible = list;
  return list;
}

// ───────────────────────────── Okładki (miniatury) ─────────────────────────────
const coverQueue = [];
let coverBusy = 0;
function wantCover(id, size = 96) {
  const key = id + ':' + size;
  if (S.covers.has(key)) return S.covers.get(key);
  S.covers.set(key, undefined);
  coverQueue.push([id, size]);
  pumpCovers();
  return undefined;
}
function pumpCovers() {
  while (coverBusy < 4 && coverQueue.length) {
    const [id, size] = coverQueue.pop();
    coverBusy++;
    invoke('get_cover', { id, size })
      .then((url) => {
        S.covers.set(id + ':' + size, url || null);
        $$(`[data-cover="${id}:${size}"]`).forEach((el) => setCoverEl(el, url));
      })
      .catch(() => S.covers.set(id + ':' + size, null))
      .finally(() => { coverBusy--; pumpCovers(); });
  }
}
function setCoverEl(el, url) {
  if (!url) return;
  el.innerHTML = `<img src="${url}" alt="" loading="lazy">`;
}
function coverHTML(x, size = 96, cls = 'cover') {
  if (!x.cover) return `<div class="${cls}">${icon('music')}</div>`;
  const url = wantCover(x.id, size);
  return `<div class="${cls}" data-cover="${x.id}:${size}">${url ? `<img src="${url}" alt="">` : icon('disc')}</div>`;
}

// ───────────────────────────── Nawigacja ─────────────────────────────
const NAV = [
  { id: 'library', icon: 'music', label: 'nav_library' },
  { id: 'review', icon: 'sparkles', label: 'nav_review' },
  { id: 'duplicates', icon: 'duplicate', label: 'nav_duplicates' },
  { id: 'history', icon: 'history', label: 'nav_history' },
  { id: 'settings', icon: 'settings', label: 'nav_settings' },
];
function go(view) {
  S.view = view;
  if (view === 'history') loadHistory();
  if (view === 'duplicates') loadDuplicates();
  renderAll();
}
function navBadge(id) {
  if (id === 'review') { const n = [...S.proposals.keys()].filter((k) => S.byId.get(k)?.status === 'proposed').length; return n ? `<span class="badge acc">${n}</span>` : ''; }
  if (id === 'duplicates' && S.dups?.length) return `<span class="badge">${S.dups.length}</span>`;
  return '';
}
function renderSidebar() {
  const recent = (S.settings?.recentFolders || []).slice(0, 5);
  $('#sidebar').innerHTML = `
    <div class="brand">${LOGO}<div><b>MusicTagger</b><small>${esc(t('tagline'))}</small></div></div>
    ${NAV.map((n) => `<button class="nav-item ${S.view === n.id ? 'active' : ''}" data-nav="${n.id}" title="${esc(t(n.label))}">${icon(n.icon)}<span>${esc(t(n.label))}</span><i data-badge="${n.id}">${navBadge(n.id)}</i></button>`).join('')}
    ${recent.length ? `<div class="sidebar-sep r"></div><div class="recent"><div class="dim" style="font-size:11px;padding:4px 12px;text-transform:uppercase;letter-spacing:.6px">${esc(t('recent'))}</div>
      ${recent.map((f) => `<button class="nav-item" data-open="${esc(f)}" title="${esc(f)}">${icon('folder')}<span class="ellipsis">${esc(baseName(f))}</span></button>`).join('')}</div>` : ''}
    <div class="sidebar-foot"><button class="nav-item" data-theme-toggle title="${esc(t('theme'))}">${icon(S.settings?.theme === 'light' ? 'moon' : 'sun')}<span>${esc(t(S.settings?.theme === 'light' ? 'darkMode' : 'lightMode'))}</span></button>
    <span class="txt">v${esc(S.info.version)}</span></div>`;
  $('#bottomnav').innerHTML = NAV.map((n) => `<button class="nav-item ${S.view === n.id ? 'active' : ''}" data-nav="${n.id}">${icon(n.icon)}<span>${esc(t(n.label + '_s'))}</span><i data-badge="${n.id}">${navBadge(n.id)}</i></button>`).join('');
}
function renderNavBadges() {
  $$('[data-badge]').forEach((el) => { el.innerHTML = navBadge(el.dataset.badge); });
}

function renderTopbar() {
  const f = S.folder;
  $('#topbar').innerHTML = `
    <button class="folder-btn" id="pick-folder" title="${esc(f || t('chooseFolder'))}">${icon(f ? 'folderOpen' : 'folder')}
      <span class="path">${esc(f ? baseName(f) : t('chooseFolder'))}</span>${icon('chevronDown', 'dim')}</button>
    ${S.view === 'library' && S.tracks.length ? `<div class="search">${icon('search', 'dim')}<input id="q" placeholder="${esc(t('searchPh'))}" value="${esc(S.query)}"><span class="kbd">Ctrl F</span></div>` : '<div class="grow"></div>'}
    ${f ? `<button class="btn icon ghost" id="rescan" title="${esc(t('rescan'))}">${icon('refresh')}</button>` : ''}
    ${S.view === 'library' && S.tracks.length ? `<button class="btn primary" id="smart-all">${icon('sparkles')}<span class="lbl">${esc(t('smartFix'))}</span></button>` : ''}`;
}

function renderJobbar() {
  const bar = $('#jobbar');
  const sc = S.scan, j = S.job;
  if (sc && sc.phase !== 'done') {
    bar.hidden = false;
    const pct = sc.total ? (sc.done / sc.total) * 100 : 0;
    bar.innerHTML = `${icon('refresh', 'spin')}<span>${esc(sc.total ? t('scanning', { done: sc.done, total: sc.total }) : t('collecting'))}</span>
      <div class="progress ${sc.total ? '' : 'indet'}"><i style="width:${pct}%"></i></div>
      <button class="btn sm ghost" id="cancel-job">${icon('stop')}${esc(t('stop'))}</button>`;
  } else if (j && j.running) {
    bar.hidden = false;
    const pct = j.total ? (j.done / j.total) * 100 : 0;
    bar.innerHTML = `${icon('sparkles', 'spin')}<span>${esc(t(j.kind === 'apply' ? 'applying' : 'analyzing', { done: j.done, total: j.total }))}</span>
      <div class="progress"><i style="width:${pct}%"></i></div><span class="muted">${Math.round(pct)}%</span>
      <button class="btn sm ghost" id="cancel-job">${icon('stop')}${esc(t('stop'))}</button>`;
  } else bar.hidden = true;
}

// ───────────────────────────── Widoki ─────────────────────────────
function renderAll() {
  document.documentElement.lang = currentLang();
  renderSidebar();
  renderTopbar();
  renderJobbar();
  renderView();
  renderDrawer();
  renderSelbar();
}
function renderView() {
  const v = $('#view');
  switch (S.view) {
    case 'review': return renderReview(v);
    case 'duplicates': return renderDuplicates(v);
    case 'history': return renderHistory(v);
    case 'settings': return renderSettings(v);
    default: return renderLibrary(v);
  }
}

function counts() {
  const c = { all: S.tracks.length };
  for (const k of Object.keys(FILTERS)) if (k !== 'all') c[k] = 0;
  for (const x of S.tracks) for (const k of Object.keys(FILTERS)) if (k !== 'all' && FILTERS[k](x)) c[k]++;
  return c;
}
function updateCounts() {
  const c = counts();
  $$('[data-filter] .n').forEach((el) => { el.textContent = c[el.parentElement.dataset.filter]; });
  const avg = S.tracks.length ? Math.round(S.tracks.reduce((a, x) => a + x.score, 0) / S.tracks.length) : 0;
  const ring = $('.ring .fg');
  if (ring) { ring.style.strokeDashoffset = 163.4 * (1 - avg / 100); $('.ring text').textContent = avg + '%'; }
  $$('[data-stat]').forEach((el) => { el.textContent = c[el.dataset.stat]; });
}

function renderEmpty(v) {
  const recent = S.settings?.recentFolders || [];
  v.innerHTML = `<div class="empty">
    ${!S.storageOk ? `<div class="card" style="max-width:520px;text-align:left;padding:14px 18px;border-color:var(--warn)"><h2>${icon('shield')}${esc(t('permTitle'))}</h2><p class="muted" style="line-height:1.5;margin:4px 0 12px">${esc(t('permText'))}</p><button class="btn sm" id="perm-check">${icon('refresh')}${esc(t('permCheck'))}</button></div>` : ''}
    <div class="hero-logo">${LOGO}</div>
    <h1>${esc(t('welcomeTitle'))}</h1>
    <p>${esc(t('welcomeText'))}</p>
    <div class="row wrap" style="justify-content:center">
      <button class="btn primary lg" id="pick-folder2">${icon('folderOpen')}${esc(t('chooseFolder'))}</button>
      ${isAndroid() ? '' : `<span class="dim">${esc(t('orDrop'))}</span>`}
    </div>
    ${recent.length ? `<div class="recent-list"><div class="dim" style="font-size:12px;text-align:left">${esc(t('recent'))}</div>${recent.map((f) => `<button class="btn" data-open="${esc(f)}">${icon('folder')}<span class="ellipsis grow" style="text-align:left">${esc(f)}</span>${icon('chevronRight', 'dim')}</button>`).join('')}</div>` : ''}
    <div class="feature-grid">
      <div class="feature">${icon('tag', 'lg')}<b>${esc(t('feat1'))}</b><span>${esc(t('feat1d'))}</span></div>
      <div class="feature">${icon('image', 'lg')}<b>${esc(t('feat2'))}</b><span>${esc(t('feat2d'))}</span></div>
      <div class="feature">${icon('lyrics', 'lg')}<b>${esc(t('feat3'))}</b><span>${esc(t('feat3d'))}</span></div>
    </div></div>`;
}

function renderLibrary(v) {
  if (!S.tracks.length) {
    if (S.scan && S.scan.phase !== 'done') {
      v.innerHTML = `<div class="empty">${icon('refresh', 'xl spin')}<h1>${esc(t('scanningTitle'))}</h1><p>${esc(S.folder || '')}</p></div>`;
      return;
    }
    if (S.folder) {
      v.innerHTML = `<div class="empty">${icon('music', 'xl')}<h1>${esc(t('noAudio'))}</h1><p>${esc(t('noAudioText'))}</p><button class="btn primary" id="pick-folder2">${icon('folderOpen')}${esc(t('chooseFolder'))}</button></div>`;
      return;
    }
    return renderEmpty(v);
  }
  const c = counts();
  const avg = Math.round(S.tracks.reduce((a, x) => a + x.score, 0) / S.tracks.length);
  const chip = (k, ic) => `<button class="chip ${S.filter === k ? 'on' : ''}" data-filter="${k}">${ic ? icon(ic) : ''}${esc(t('flt_' + k))}<span class="n">${c[k]}</span></button>`;
  const sortOpt = (k) => `<option value="${k}" ${S.sort.key === k ? 'selected' : ''}>${esc(t('sort_' + k))}</option>`;
  const albumName = S.album ? (S.album.startsWith('\u0002') ? baseName(S.album.slice(1)) : S.album.split('\u0001')[1]) : '';
  v.innerHTML = `
    <div class="stats">
      <div class="stat main"><svg class="ring" viewBox="0 0 64 64"><defs><linearGradient id="ringgrad"><stop offset="0" stop-color="var(--acc)"/><stop offset="1" stop-color="var(--acc2)"/></linearGradient></defs><circle class="bg" cx="32" cy="32" r="26"/><circle class="fg" cx="32" cy="32" r="26" stroke-dasharray="163.4" style="stroke-dashoffset:${163.4 * (1 - avg / 100)}"/><text x="32" y="33">${avg}%</text></svg>
        <div class="grow"><div class="v">${S.tracks.length}</div><div class="l">${esc(t('statTracks'))} · ${esc(t('statComplete'))}</div></div></div>
      <div class="stat" data-goto="missingTags"><div class="ic-wrap" style="color:var(--warn)">${icon('tag')}</div><div><div class="v" data-stat="missingTags">${c.missingTags}</div><div class="l">${esc(t('flt_missingTags'))}</div></div></div>
      <div class="stat" data-goto="noCover"><div class="ic-wrap" style="color:var(--bad)">${icon('image')}</div><div><div class="v" data-stat="noCover">${c.noCover}</div><div class="l">${esc(t('flt_noCover'))}</div></div></div>
      <div class="stat hide-sm" data-goto="noLyrics"><div class="ic-wrap" style="color:var(--info)">${icon('lyrics')}</div><div><div class="v" data-stat="noLyrics">${c.noLyrics}</div><div class="l">${esc(t('flt_noLyrics'))}</div></div></div>
      <div class="stat hide-sm hide-md" data-goto="proposed"><div class="ic-wrap" style="color:var(--acc)">${icon('sparkles')}</div><div><div class="v" data-stat="proposed">${c.proposed}</div><div class="l">${esc(t('flt_proposed'))}</div></div></div>
    </div>
    <div class="toolbar">
      <div class="chips">
        ${S.album ? `<button class="chip on" id="clear-album">${icon('disc')}${esc(albumName)}${icon('x')}</button>` : ''}
        ${chip('all')}${chip('missingTags')}${chip('noCover')}${chip('noLyrics')}${chip('noSynced')}${chip('proposed')}${chip('problems')}${chip('complete')}
      </div>
      <select class="sel" id="sort" title="${esc(t('sortBy'))}">${['path', 'title', 'artist', 'album', 'score', 'duration', 'modified'].map(sortOpt).join('')}</select>
      <button class="btn icon" id="sort-dir" title="${esc(t('sortDir'))}">${icon('sort')}</button>
      <div class="btn-group"><button class="btn icon ${S.mode === 'list' ? 'on' : ''}" data-mode="list" title="${esc(t('viewList'))}">${icon('list')}</button><button class="btn icon ${S.mode === 'albums' ? 'on' : ''}" data-mode="albums" title="${esc(t('viewAlbums'))}">${icon('grid')}</button></div>
      <button class="btn icon" id="more-actions" title="${esc(t('more'))}">${icon('more')}</button>
    </div>
    <div id="listwrap" style="flex:1;min-height:0;display:flex;flex-direction:column"></div>`;
  computeVisible();
  if (S.mode === 'albums' && !S.album) renderAlbums($('#listwrap'));
  else renderList($('#listwrap'));
}

function renderList(el) {
  const all = S.visible.length && S.visible.every((x) => S.selected.has(x.id));
  const some = !all && S.visible.some((x) => S.selected.has(x.id));
  const sh = (k, label, cls = '') => `<span class="sortable ${cls}" data-sort="${k}">${esc(label)}${S.sort.key === k ? (S.sort.dir > 0 ? ' ↑' : ' ↓') : ''}</span>`;
  el.innerHTML = `<div class="list-head"><button class="cb ${all ? 'on' : some ? 'part' : ''}" id="sel-all" title="Ctrl+A">${icon('check')}</button><span></span>
    ${sh('title', t('colTitle'))}<span class="c-album">${sh('album', t('colAlbum'))}</span><span>${esc(t('colMissing'))}</span><span class="c-score-h">${sh('score', t('colScore'))}</span><span>${esc(t('colStatus'))}</span></div>
    <div class="list-scroll" id="lscroll"><div class="list-inner" id="linner"></div></div>`;
  if (!S.visible.length) {
    $('#lscroll').innerHTML = `<div class="empty">${icon('filter', 'xl')}<p>${esc(t('nothingHere'))}</p></div>`;
    return;
  }
  const sc = $('#lscroll');
  sc.addEventListener('scroll', () => requestAnimationFrame(paintRows), { passive: true });
  bindRowEvents($('#linner'));
  paintRows();
}

function rowH() { return parseInt(getComputedStyle(document.documentElement).getPropertyValue('--row-h')) || 62; }
function missIcons(x) {
  const m = x.missing;
  const tagsCls = CORE.every((f) => !m.includes(f)) && !m.includes('genre') ? 'ok' : x.tags.title && x.tags.artist ? 'half' : 'no';
  const coverCls = m.includes('cover') ? 'no' : m.includes('coverSmall') ? 'half' : 'ok';
  const lyrCls = m.includes('lyrics') ? 'no' : m.includes('syncedLyrics') ? 'half' : 'ok';
  const tip = (k, c) => esc(t('miss_' + k + '_' + c));
  return `<div class="miss"><span class="m ${tagsCls}" title="${tip('tags', tagsCls)}">${icon('tag')}</span><span class="m ${coverCls}" title="${tip('cover', coverCls)}">${icon('image')}</span><span class="m ${lyrCls}" title="${tip('lyrics', lyrCls)}">${icon('lyrics')}</span></div>`;
}
function statusHTML(x) {
  const p = S.proposals.get(x.id);
  let label = t('st_' + x.status);
  if (x.status === 'proposed' && p) label = `${Math.round(p.confidence * 100)}%`;
  return `<span class="status ${x.status}" title="${esc(x.error || t('st_' + x.status))}"><span class="dot"></span><span class="lbl">${esc(label)}</span></span>`;
}
function rowHTML(x, top) {
  const title = x.tags.title || x.fileName;
  const sub = [x.tags.artist || t('unknownArtist'), x.tags.year].filter(Boolean).join(' · ');
  return `<div class="trow ${S.selected.has(x.id) ? 'sel' : ''} ${S.active === x.id ? 'active' : ''}" style="top:${top}px" data-id="${x.id}">
    <button class="cb ${S.selected.has(x.id) ? 'on' : ''}" data-cb>${icon('check')}</button>
    ${coverHTML(x)}
    <div class="grow"><div class="t1 ellipsis" title="${esc(x.path)}">${esc(title)}</div><div class="t2 ellipsis">${esc(sub)}</div></div>
    <div class="c-album grow"><div class="ellipsis">${esc(x.tags.album || '—')}</div><div class="t2 ellipsis">${esc(x.format)} · ${fmtDur(x.duration)}${x.tags.track ? ' · #' + x.tags.track : ''}</div></div>
    <div class="c-miss">${missIcons(x)}</div>
    <div class="c-score score"><div class="bar"><i style="width:${x.score}%;background:${scoreColor(x.score)}"></i></div><b>${x.score}</b></div>
    <div class="c-status">${statusHTML(x)}</div></div>`;
}
function paintRows() {
  const sc = $('#lscroll'), inner = $('#linner');
  if (!sc || !inner) return;
  const h = rowH();
  const list = S.visible;
  inner.style.height = list.length * h + 140 + 'px';
  const start = Math.max(0, Math.floor(sc.scrollTop / h) - 6);
  const end = Math.min(list.length, Math.ceil((sc.scrollTop + sc.clientHeight) / h) + 6);
  let html = '';
  for (let i = start; i < end; i++) html += rowHTML(list[i], i * h);
  inner.innerHTML = html;
  const sa = $('#sel-all');
  if (sa) {
    const all = list.length && list.every((x) => S.selected.has(x.id));
    const some = !all && list.some((x) => S.selected.has(x.id));
    sa.className = 'cb ' + (all ? 'on' : some ? 'part' : '');
  }
}
function bindRowEvents(inner) {
  let pressTimer = null, longPressed = false;
  inner.addEventListener('pointerdown', (e) => {
    const row = e.target.closest('.trow');
    if (!row || e.pointerType === 'mouse') return;
    longPressed = false;
    pressTimer = setTimeout(() => { longPressed = true; toggleSel(row.dataset.id, false); navigator.vibrate?.(20); }, 450);
  });
  const cancel = () => clearTimeout(pressTimer);
  inner.addEventListener('pointerup', cancel);
  inner.addEventListener('pointermove', cancel);
  inner.addEventListener('pointercancel', cancel);
  inner.addEventListener('click', (e) => {
    const row = e.target.closest('.trow');
    if (!row) return;
    const id = row.dataset.id;
    if (longPressed) { longPressed = false; return; }
    if (e.target.closest('[data-cb]') || e.ctrlKey || e.metaKey) return toggleSel(id, e.shiftKey);
    if (e.shiftKey) return toggleSel(id, true);
    if (S.selected.size && isMobile()) return toggleSel(id, false);
    openDrawer(id);
  });
}
function toggleSel(id, range) {
  if (range && S.anchor) {
    const ids = S.visible.map((x) => x.id);
    const a = ids.indexOf(S.anchor), b = ids.indexOf(id);
    if (a >= 0 && b >= 0) { for (let i = Math.min(a, b); i <= Math.max(a, b); i++) S.selected.add(ids[i]); }
  } else {
    S.selected.has(id) ? S.selected.delete(id) : S.selected.add(id);
    S.anchor = id;
  }
  paintRows();
  renderSelbar();
}

function renderAlbums(el) {
  const groups = new Map();
  for (const x of S.visible) {
    const k = albumKey(x);
    if (!groups.has(k)) groups.set(k, []);
    groups.get(k).push(x);
  }
  const arr = [...groups.entries()].sort((a, b) => a[0].localeCompare(b[0]));
  el.innerHTML = `<div class="albums">${arr.map(([k, list]) => {
    const withCover = list.find((x) => x.cover) || list[0];
    const avg = Math.round(list.reduce((a, x) => a + x.score, 0) / list.length);
    const title = list[0].tags.album || baseName(list[0].folder);
    const artist = list[0].tags.albumArtist || list[0].tags.artist || t('unknownArtist');
    return `<div class="album" data-album="${esc(k)}">${coverHTML(withCover, 300)}
      <div class="t1 ellipsis" title="${esc(title)}">${esc(title)}</div><div class="t2 ellipsis">${esc(artist)}</div>
      <div class="meta"><span class="badge">${list.length} ${esc(t('tracksShort'))}</span><span class="badge" style="color:${scoreColor(avg)}">${avg}%</span></div></div>`;
  }).join('')}</div>`;
}

function renderSelbar() {
  const bar = $('#selbar');
  const n = S.selected.size;
  document.body.classList.toggle('selbar-open', !!n && S.view === 'library');
  if (!n || S.view !== 'library') { bar.hidden = true; return; }
  const withProp = [...S.selected].filter((id) => S.proposals.has(id)).length;
  bar.hidden = false;
  bar.innerHTML = `<span class="count">${esc(t('selected', { n }))}</span>
    <button class="btn primary sm" data-act="analyze">${icon('sparkles')}<span class="lbl">${esc(t('smartFixSel'))}</span></button>
    ${withProp ? `<button class="btn sm" data-act="apply">${icon('check')}<span class="lbl">${esc(t('applyN', { n: withProp }))}</span></button>` : ''}
    <button class="btn sm" data-act="batch">${icon('edit')}<span class="lbl">${esc(t('batchEdit'))}</span></button>
    <button class="btn sm" data-act="rename">${icon('rename')}<span class="lbl">${esc(t('rename'))}</span></button>
    <button class="btn sm ghost icon" data-act="clear" title="${esc(t('clearSel'))}">${icon('x')}</button>`;
}

// ───────────────────────────── Panel szczegółów ─────────────────────────────
function openDrawer(id, tab) {
  const wasOpen = !!S.active;
  S.active = id;
  const x = S.byId.get(id);
  S.tab = tab || (S.proposals.has(id) ? 'proposal' : (S.tab === 'proposal' ? 'edit' : S.tab || 'edit'));
  if (S.tab === 'proposal' && !S.proposals.has(id)) S.tab = 'edit';
  if (!wasOpen) pushBack();
  stopAudio();
  renderDrawer();
  paintRows();
  if (x && isMobile()) document.activeElement?.blur();
}
function closeDrawer(fromBack) {
  S.active = null;
  stopAudio();
  renderDrawer();
  paintRows();
  if (!fromBack && backDepth) history.back();
}
function renderDrawer() {
  const d = $('#drawer');
  const x = S.active && S.byId.get(S.active);
  document.body.classList.toggle('drawer-open', !!x);
  if (!x) { d.classList.remove('open'); d.setAttribute('aria-hidden', 'true'); d.innerHTML = ''; return; }
  d.classList.add('open');
  d.setAttribute('aria-hidden', 'false');
  const p = S.proposals.get(x.id);
  const tabs = [
    p ? ['proposal', 'sparkles', 'tabProposal'] : null,
    ['edit', 'tag', 'tabTags'],
    ['cover', 'image', 'tabCover'],
    ['lyrics', 'lyrics', 'tabLyrics'],
    ['file', 'file', 'tabFile'],
  ].filter(Boolean);
  d.innerHTML = `<div class="drawer-inner">
    <div class="drawer-head">${coverHTML(x, 300)}
      <div class="grow"><div class="row" style="gap:6px">${statusHTML(x)}</div>
        <h2>${esc(x.tags.title || x.fileName)}</h2>
        <div class="muted ellipsis">${esc(x.tags.artist || t('unknownArtist'))}${x.tags.album ? ' — ' + esc(x.tags.album) : ''}</div>
        <div class="score" style="margin-top:8px;max-width:220px"><div class="bar"><i style="width:${x.score}%;background:${scoreColor(x.score)}"></i></div><b>${x.score}</b></div>
      </div>
      <div class="col" style="gap:4px"><button class="btn icon ghost" id="d-close" title="Esc">${icon('x')}</button>
        <button class="btn icon ghost" id="d-prev" title="↑">${icon('chevronLeft')}</button><button class="btn icon ghost" id="d-next" title="↓">${icon('chevronRight')}</button></div>
    </div>
    <div class="tabs">${tabs.map(([k, ic, l]) => `<button class="tab ${S.tab === k ? 'on' : ''}" data-tab="${k}">${icon(ic)}${esc(t(l))}</button>`).join('')}</div>
    <div class="drawer-body" id="dbody"></div>
    <div class="drawer-foot" id="dfoot"></div></div>`;
  const body = $('#dbody'), foot = $('#dfoot');
  ({ proposal: tabProposal, edit: tabEdit, cover: tabCover, lyrics: tabLyrics, file: tabFile }[S.tab] || tabEdit)(x, body, foot, p);
}
function stepActive(dir) {
  const ids = S.visible.map((x) => x.id);
  const i = ids.indexOf(S.active);
  const n = ids[i + dir];
  if (n) {
    openDrawer(n, S.tab === 'proposal' ? undefined : S.tab);
    const sc = $('#lscroll');
    if (sc) {
      const top = ids.indexOf(n) * rowH();
      if (top < sc.scrollTop || top > sc.scrollTop + sc.clientHeight - rowH()) sc.scrollTop = top - sc.clientHeight / 2;
    }
  }
}

// Zakładka: Propozycja
const METHOD = { search: 'mSearch', fingerprint: 'mFingerprint', mbid: 'mMbid', manual: 'mManual' };
function tabProposal(x, body, foot, p) {
  const pct = Math.round(p.confidence * 100);
  const nothing = !p.changes.length && !p.covers.length && !p.lyrics;
  const alt = p.alternatives || [];
  body.innerHTML = `
    <div class="conf"><div class="big" style="color:${scoreColor(pct)}">${pct}%</div><div class="grow"><b>${esc(t('confidence'))}</b><div class="muted" style="font-size:12.5px">${esc(t(METHOD[p.method] || 'mSearch'))}</div></div>
      ${p.method === 'fingerprint' ? `<span class="badge acc">${icon('fingerprint')}AcoustID</span>` : ''}</div>
    ${x.error ? `<div class="toggle-line" style="border-color:var(--bad);margin-bottom:12px">${icon('alert')}<span class="selectable">${esc(x.error)}</span></div>` : ''}
    ${nothing ? `<p class="muted">${esc(t('nothingToChange'))}</p>` : ''}
    ${p.changes.length ? `<div class="section-title">${icon('tag')}${esc(t('tagChanges'))} <span class="badge">${p.changes.filter((c) => c.enabled).length}/${p.changes.length}</span></div>
    <div class="diff">${p.changes.map((c, i) => `<div class="diff-row ${c.enabled ? '' : 'off'}" data-ch="${i}">
      <button class="cb ${c.enabled ? 'on' : ''}">${icon('check')}</button><div class="k">${esc(fieldLabel(c.field))}</div>
      <div>${c.old ? `<div class="old">${esc(c.old)}</div>` : ''}<div class="new" contenteditable="true" spellcheck="false" data-edit="${i}">${esc(c.new ?? '')}</div></div></div>`).join('')}</div>` : ''}
    ${p.covers.length ? `<div class="section-title">${icon('image')}${esc(t('coverSec'))}</div>
      <div class="toggle-line" data-toggle="cover" style="margin-bottom:10px"><button class="cb ${p.coverEnabled ? 'on' : ''}">${icon('check')}</button><span class="grow">${esc(x.cover ? t('replaceCover', { w: x.cover.width, h: x.cover.height }) : t('addCover'))}</span></div>
      <div class="covers-pick">${p.covers.map((c, i) => `<div class="cpick ${p.coverChoice === i ? 'on' : ''}" data-cpick="${i}" title="${esc(c.source)}"><img src="${esc(c.thumb)}" alt="" loading="lazy" onerror="this.parentElement.style.display='none'"><span class="src">${esc(c.source)}</span></div>`).join('')}</div>` : ''}
    ${p.lyrics ? `<div class="section-title">${icon('lyrics')}${esc(t('lyricsSec'))} ${p.lyrics.synced ? `<span class="badge ok">${icon('clock')}${esc(t('synced'))}</span>` : `<span class="badge">${esc(t('plain'))}</span>`}</div>
      ${p.lyrics.instrumental ? `<p class="muted">${esc(t('instrumental'))}</p>` : `<div class="toggle-line" data-toggle="lyrics" style="margin-bottom:10px"><button class="cb ${p.lyricsEnabled ? 'on' : ''}">${icon('check')}</button><span class="grow">${esc(x.lyrics ? t('replaceLyrics') : t('addLyrics'))}</span></div>
      <div class="lyrics-pre">${esc(p.lyrics.synced || p.lyrics.plain || '')}</div>`}` : ''}
    ${alt.length > 1 ? `<div class="section-title">${icon('layers')}${esc(t('alternatives'))}</div>
      <div class="col" style="gap:6px">${alt.map((a, i) => `<div class="alt ${p.chosen === i ? 'on' : ''}" data-alt="${i}">${a.coverThumb ? `<img src="${esc(a.coverThumb)}" alt="" loading="lazy" onerror="this.style.visibility='hidden'">` : `<div class="cover" style="width:40px;height:40px">${icon('disc')}</div>`}
        <div class="grow"><div class="ellipsis"><b>${esc(a.fields.title || '')}</b> — ${esc(a.fields.artist || '')}</div>
        <div class="muted ellipsis" style="font-size:12px">${esc([a.fields.album, a.fields.year, a.releaseType, ...(a.secondaryTypes || [])].filter(Boolean).join(' · '))}</div></div>
        <div class="col" style="gap:3px;align-items:flex-end"><span class="badge">${esc(a.source)}</span><span class="badge" style="color:${scoreColor(a.score * 100)}">${Math.round(a.score * 100)}%</span></div></div>`).join('')}</div>` : ''}
    ${(p.notes || []).filter((n) => n !== 'instrumental').length ? `<div class="section-title">${icon('info')}${esc(t('notes'))}</div><div class="muted selectable" style="font-size:12px">${p.notes.filter((n) => n !== 'instrumental').map(esc).join('<br>')}</div>` : ''}`;
  foot.innerHTML = `<button class="btn ghost" id="p-manual">${icon('search')}${esc(t('manualSearch'))}</button><div class="grow"></div>
    <button class="btn" id="p-discard">${icon('x')}${esc(t('discard'))}</button>
    <button class="btn primary" id="p-apply" ${nothing ? 'disabled' : ''}>${icon('check')}${esc(t('apply'))}</button>`;
  const save = debounce(() => invoke('update_proposal', { proposal: p }), 250);
  $$('[data-ch]', body).forEach((row) => {
    $('.cb', row).onclick = (e) => { e.stopPropagation(); const c = p.changes[+row.dataset.ch]; c.enabled = !c.enabled; row.classList.toggle('off', !c.enabled); e.currentTarget.classList.toggle('on', c.enabled); save(); };
  });
  $$('[data-edit]', body).forEach((el) => {
    el.addEventListener('input', () => { const c = p.changes[+el.dataset.edit]; c.new = el.textContent.trim() || null; c.enabled = true; el.closest('.diff-row').classList.remove('off'); $('.cb', el.closest('.diff-row')).classList.add('on'); save(); });
    el.addEventListener('keydown', (e) => { if (e.key === 'Enter') { e.preventDefault(); el.blur(); } });
  });
  $$('[data-toggle]', body).forEach((row) => {
    row.onclick = () => { const k = row.dataset.toggle === 'cover' ? 'coverEnabled' : 'lyricsEnabled'; p[k] = !p[k]; $('.cb', row).classList.toggle('on', p[k]); save(); };
  });
  $$('[data-cpick]', body).forEach((el) => {
    el.onclick = () => { p.coverChoice = +el.dataset.cpick; p.coverEnabled = true; $$('[data-cpick]', body).forEach((e) => e.classList.toggle('on', e === el)); $('[data-toggle=cover] .cb', body)?.classList.add('on'); save(); };
  });
  $$('[data-alt]', body).forEach((el) => {
    el.onclick = async () => {
      el.style.opacity = '.5';
      try {
        const np = await invoke('choose_alternative', { id: x.id, index: +el.dataset.alt });
        S.proposals.set(x.id, np);
        renderDrawer();
      } catch (e) { toast(errMsg(e), 'bad'); el.style.opacity = ''; }
    };
  });
  $('#p-apply').onclick = () => applyIds([x.id]);
  $('#p-discard').onclick = () => { invoke('discard_proposals', { ids: [x.id] }); S.proposals.delete(x.id); x.status = 'idle'; S.tab = 'edit'; scheduleRender('rows'); renderDrawer(); };
  $('#p-manual').onclick = () => manualSearch(x);
}

// Zakładka: Tagi (edycja)
function tabEdit(x, body, foot) {
  const v = (f) => x.tags[f] ?? '';
  const inp = (f, cls = '') => `<div class="field ${cls}"><label>${esc(fieldLabel(f))}${!x.tags[f] && CORE.includes(f) ? `<span class="badge warn">${esc(t('missing'))}</span>` : ''}</label><input class="inp ${!x.tags[f] ? 'missing' : ''}" data-f="${f}" value="${esc(v(f))}"></div>`;
  body.innerHTML = `<div class="form">
    ${inp('title', 'full')}${inp('artist')}${inp('albumArtist')}${inp('album', 'full')}
    <div class="field"><label>${esc(t('f_track'))}</label><div class="pair"><input class="inp" data-f="track" value="${esc(v('track'))}" inputmode="numeric"><span class="dim">/</span><input class="inp" data-f="trackTotal" value="${esc(v('trackTotal'))}" inputmode="numeric"></div></div>
    <div class="field"><label>${esc(t('f_disc'))}</label><div class="pair"><input class="inp" data-f="disc" value="${esc(v('disc'))}" inputmode="numeric"><span class="dim">/</span><input class="inp" data-f="discTotal" value="${esc(v('discTotal'))}" inputmode="numeric"></div></div>
    ${inp('year')}${inp('genre')}${inp('composer')}${inp('label')}${inp('isrc')}${inp('bpm')}${inp('comment', 'full')}
    </div>
    <div class="section-title" style="margin-top:20px">${icon('sparkles')}${esc(t('smart'))}</div>
    <div class="row wrap"><button class="btn sm" id="e-analyze">${icon('sparkles')}${esc(t('smartFixOne'))}</button><button class="btn sm" id="e-manual">${icon('search')}${esc(t('manualSearch'))}</button><button class="btn sm" id="e-fromname">${icon('file')}${esc(t('fromFilename'))}</button><button class="btn sm" id="e-case">${icon('rename')}${esc(t('fixCase'))}</button></div>`;
  foot.innerHTML = `<span class="muted grow" id="e-dirty"></span><button class="btn" id="e-reset">${esc(t('reset'))}</button><button class="btn primary" id="e-save" disabled>${icon('check')}${esc(t('save'))}</button>`;
  const changed = () => {
    const ch = {};
    $$('[data-f]', body).forEach((el) => {
      const f = el.dataset.f, nv = el.value.trim(), ov = String(x.tags[f] ?? '');
      el.classList.toggle('changed', nv !== ov);
      if (nv !== ov) ch[f] = nv || null;
    });
    const n = Object.keys(ch).length;
    $('#e-save').disabled = !n;
    $('#e-dirty').textContent = n ? t('unsaved', { n }) : '';
    return ch;
  };
  body.addEventListener('input', changed);
  body.addEventListener('keydown', (e) => { if (e.key === 'Enter' && e.target.matches('.inp')) $('#e-save').click(); });
  $('#e-reset').onclick = () => renderDrawer();
  $('#e-save').onclick = async () => {
    const fields = changed();
    try { await invoke('save_edits', { req: { ids: [x.id], fields } }); toast(t('saved'), 'ok'); }
    catch (e) { toast(errMsg(e), 'bad'); }
  };
  $('#e-analyze').onclick = () => analyzeIds([x.id]);
  $('#e-manual').onclick = () => manualSearch(x);
  $('#e-fromname').onclick = () => {
    const stem = x.fileName.replace(/\.[^.]+$/, '').replace(/_/g, ' ');
    const m = stem.replace(/^\s*\d{1,3}\s*[-.)_ ]\s*/, '').split(/\s+[-–]\s+/);
    const set = (f, val) => { const el = $(`[data-f="${f}"]`, body); if (el && val) el.value = val.trim(); };
    if (m.length >= 2) { set('artist', m[0]); set('title', m.slice(1).join(' - ')); } else set('title', m[0]);
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

// Zakładka: Okładka
function tabCover(x, body, foot) {
  body.innerHTML = `<div class="bigcover" id="bigc">${icon('image')}</div>
    <div class="row wrap" style="margin-top:10px;justify-content:space-between"><span class="muted">${x.cover ? `${x.cover.width}×${x.cover.height} px · ${esc(x.cover.mime.replace('image/', '').toUpperCase())} · ${fmtSize(x.cover.bytes)}` : esc(t('noCover'))}</span>
      ${x.cover && x.cover.width < (S.settings?.minCoverSize || 500) ? `<span class="badge warn">${esc(t('coverSmall'))}</span>` : x.cover ? `<span class="badge ok">${esc(t('coverOk'))}</span>` : ''}</div>
    <div class="section-title" style="margin-top:18px">${icon('image')}${esc(t('changeCover'))}</div>
    <div class="row wrap">
      <button class="btn sm" id="c-search">${icon('search')}${esc(t('coverSearch'))}</button>
      ${isAndroid() ? '' : `<button class="btn sm" id="c-file">${icon('upload')}${esc(t('coverFile'))}</button>`}
      <button class="btn sm" id="c-url">${icon('link')}${esc(t('coverUrl'))}</button>
      ${x.cover && !isAndroid() ? `<button class="btn sm" id="c-export">${icon('download')}${esc(t('coverExport'))}</button>` : ''}
      ${x.cover ? `<button class="btn sm danger" id="c-remove">${icon('trash')}${esc(t('coverRemove'))}</button>` : ''}
    </div>
    <p class="dim" style="font-size:12px;margin-top:14px">${esc(t('coverHint', { max: S.settings?.maxCoverSize || 1200 }))}</p>`;
  foot.innerHTML = '';
  if (x.cover) invoke('get_cover', { id: x.id, size: 1200 }).then((u) => { const el = $('#bigc'); if (u && el) el.innerHTML = `<img src="${u}" alt="">`; });
  $('#c-search').onclick = () => coverSearch([x.id], [x.tags.albumArtist || x.tags.artist, x.tags.album || x.tags.title].filter(Boolean).join(' '));
  $('#c-url') .onclick = async () => {
    const url = await promptBox(t('coverUrl'), 'URL', '', 'https://…');
    if (!url) return;
    try { const b64 = await invoke('fetch_cover_b64', { url }); await setCover([x.id], b64); } catch (e) { toast(errMsg(e), 'bad'); }
  };
  if ($('#c-file')) $('#c-file').onclick = async () => {
    const path = await dialog?.open({ multiple: false, filters: [{ name: t('images'), extensions: ['jpg', 'jpeg', 'png', 'webp', 'bmp', 'gif'] }] });
    if (!path) return;
    try { const b64 = await invoke('read_image_file', { path }); await setCover([x.id], b64); } catch (e) { toast(errMsg(e), 'bad'); }
  };
  if ($('#c-export')) $('#c-export').onclick = async () => {
    const dest = await dialog?.save({ defaultPath: (x.tags.album || 'cover') + '.jpg', filters: [{ name: 'JPEG', extensions: ['jpg'] }] });
    if (!dest) return;
    try { await invoke('export_cover', { id: x.id, dest }); toast(t('exported'), 'ok'); } catch (e) { toast(errMsg(e), 'bad'); }
  };
  if ($('#c-remove')) $('#c-remove').onclick = async () => {
    if (!(await confirmBox(t('coverRemove'), t('coverRemoveQ'), t('remove'), true))) return;
    try { await invoke('save_edits', { req: { ids: [x.id], cover: { op: 'remove' } } }); toast(t('saved'), 'ok'); } catch (e) { toast(errMsg(e), 'bad'); }
  };
}
async function setCover(ids, dataUrl) {
  await invoke('save_edits', { req: { ids, cover: { op: 'set', dataB64: dataUrl } } });
  toast(t('coverSet', { n: ids.length }), 'ok');
}

// Zakładka: Tekst + karaoke
function parseLrc(text) {
  const out = [];
  for (const line of String(text || '').split(/\r?\n/)) {
    const tags = [...line.matchAll(/\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]/g)];
    if (!tags.length) continue;
    const words = line.replace(/\[[^\]]*\]/g, '').trim();
    for (const m of tags) {
      const frac = m[3] ? +m[3] / Math.pow(10, m[3].length) : 0;
      out.push({ t: +m[1] * 60 + +m[2] + frac, text: words });
    }
  }
  return out.sort((a, b) => a.t - b.t);
}
function shiftLrc(text, delta) {
  return text.replace(/\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]/g, (m, mm, ss, fr) => {
    let tt = +mm * 60 + +ss + (fr ? +fr / Math.pow(10, fr.length) : 0) + delta;
    if (tt < 0) tt = 0;
    const M = Math.floor(tt / 60), Sx = tt - M * 60;
    return `[${String(M).padStart(2, '0')}:${Sx.toFixed(2).padStart(5, '0')}]`;
  });
}
const audio = () => $('#player');
function stopAudio() { const a = audio(); if (a && !a.paused) a.pause(); }
function tabLyrics(x, body, foot) {
  const text = x.lyrics || '';
  const lines = parseLrc(text);
  body.innerHTML = `
    <div class="row wrap" style="margin-bottom:12px">${x.lyrics ? (x.lyricsSynced ? `<span class="badge ok">${icon('clock')}${esc(t('synced'))}</span>` : `<span class="badge warn">${esc(t('plain'))}</span>`) : `<span class="badge bad">${esc(t('noLyrics'))}</span>`}
      ${x.lrcFile ? `<span class="badge info">.lrc</span>` : ''}<div class="grow"></div>
      <button class="btn sm" id="l-search">${icon('search')}${esc(t('lyricsSearch'))}</button></div>
    ${lines.length ? `<div class="karaoke" id="kara"><div class="lines" id="klines">${lines.map((l, i) => `<div class="ln" data-i="${i}" data-t="${l.t}">${esc(l.text || '♪')}</div>`).join('')}</div></div>` : ''}
    <div class="player"><button class="btn icon" id="l-play">${icon('play')}</button><input type="range" id="l-seek" min="0" max="${Math.round(x.duration) || 1}" step="0.1" value="0"><span class="time" id="l-time">0:00 / ${fmtDur(x.duration)}</span></div>
    ${lines.length ? `<div class="row wrap" style="margin-bottom:10px"><span class="muted" style="font-size:12.5px">${esc(t('offset'))}</span>
      <button class="btn sm" data-shift="-0.5">−0.5 s</button><button class="btn sm" data-shift="-0.1">−0.1 s</button><button class="btn sm" data-shift="0.1">+0.1 s</button><button class="btn sm" data-shift="0.5">+0.5 s</button></div>` : ''}
    <div class="field"><label>${esc(t('lyricsEditor'))}</label><textarea class="ta" id="l-text" spellcheck="false" placeholder="${esc(t('lyricsPh'))}">${esc(text)}</textarea></div>
    ${!isAndroid() ? `<p class="dim" style="font-size:12px">${esc(t('lyricsHint'))}</p>` : ''}`;
  foot.innerHTML = `${x.lyrics ? `<button class="btn danger" id="l-del">${icon('trash')}${esc(t('remove'))}</button>` : ''}<span class="grow"></span><button class="btn" id="l-stamp" title="${esc(t('stampHint'))}">${icon('clock')}${esc(t('stamp'))}</button><button class="btn primary" id="l-save" disabled>${icon('check')}${esc(t('save'))}</button>`;
  const ta = $('#l-text');
  ta.oninput = () => { $('#l-save').disabled = ta.value === text; };
  const a = audio();
  if (a.dataset.src !== x.path) { a.src = fileSrc(x.path); a.dataset.src = x.path; }
  const playBtn = $('#l-play'), seek = $('#l-seek'), time = $('#l-time');
  const lineEls = $$('.ln', body);
  let lastIdx = -1;
  const tick = () => {
    if (!document.body.contains(seek)) return;
    seek.value = a.currentTime;
    time.textContent = `${fmtDur(a.currentTime)} / ${fmtDur(x.duration)}`;
    if (lines.length) {
      let idx = -1;
      for (let i = 0; i < lines.length; i++) if (lines[i].t <= a.currentTime + 0.15) idx = i; else break;
      if (idx !== lastIdx) {
        lastIdx = idx;
        lineEls.forEach((el, i) => { el.classList.toggle('cur', i === idx); el.classList.toggle('past', i < idx); });
        const cur = lineEls[Math.max(0, idx)];
        if (cur) $('#klines').style.transform = `translateY(${-cur.offsetTop + 130}px)`;
      }
    }
  };
  a.ontimeupdate = tick;
  a.onplay = () => { playBtn.innerHTML = icon('pause'); };
  a.onpause = () => { playBtn.innerHTML = icon('play'); };
  a.onerror = () => { playBtn.disabled = true; playBtn.title = t('cantPlay'); };
  playBtn.onclick = () => (a.paused ? a.play().catch((e) => toast(t('cantPlay') + ': ' + errMsg(e), 'bad')) : a.pause());
  seek.oninput = () => { a.currentTime = +seek.value; tick(); };
  lineEls.forEach((el) => { el.onclick = () => { a.currentTime = +el.dataset.t; tick(); if (a.paused) a.play().catch(() => {}); }; });
  $$('[data-shift]', body).forEach((b) => { b.onclick = () => { ta.value = shiftLrc(ta.value, +b.dataset.shift); $('#l-save').disabled = false; toast(t('shifted', { s: b.dataset.shift }), 'info'); }; });
  // Ręczne wstawianie znaczników czasu: kursor w linii → [mm:ss.xx] na początku, przejście do następnej.
  $('#l-stamp').onclick = () => {
    const pos = ta.selectionStart;
    const start = ta.value.lastIndexOf('\n', pos - 1) + 1;
    const tt = a.currentTime, M = Math.floor(tt / 60), Sx = tt - M * 60;
    const stamp = `[${String(M).padStart(2, '0')}:${Sx.toFixed(2).padStart(5, '0')}]`;
    const rest = ta.value.slice(start).replace(/^\[\d{1,3}:\d{2}(?:[.:]\d{1,3})?\]/, '');
    ta.value = ta.value.slice(0, start) + stamp + rest;
    const next = ta.value.indexOf('\n', start);
    const np = next < 0 ? ta.value.length : next + 1;
    ta.focus();
    ta.setSelectionRange(np, np);
    $('#l-save').disabled = false;
  };
  $('#l-save').onclick = async () => {
    try { await invoke('save_edits', { req: { ids: [x.id], lyrics: ta.value.trim() || null } }); toast(t('saved'), 'ok'); } catch (e) { toast(errMsg(e), 'bad'); }
  };
  if ($('#l-del')) $('#l-del').onclick = async () => {
    if (!(await confirmBox(t('remove'), t('lyricsRemoveQ'), t('remove'), true))) return;
    try { await invoke('save_edits', { req: { ids: [x.id], lyrics: null } }); toast(t('saved'), 'ok'); } catch (e) { toast(errMsg(e), 'bad'); }
  };
  $('#l-search').onclick = () => lyricsSearch(x, (txt) => { ta.value = txt; $('#l-save').disabled = false; });
}

// Zakładka: Plik
function tabFile(x, body, foot) {
  const kv = [
    [t('fPath'), x.path], [t('fFormat'), x.format], [t('fSize'), fmtSize(x.size)], [t('fDuration'), fmtDur(x.duration)],
    [t('fBitrate'), x.bitrate ? x.bitrate + ' kbps' : '—'], [t('fSampleRate'), x.sampleRate ? (x.sampleRate / 1000).toFixed(1) + ' kHz' : '—'],
    [t('fChannels'), x.channels ?? '—'], [t('fCover'), x.cover ? `${x.cover.width}×${x.cover.height}` : '—'],
    [t('fLyrics'), x.lyrics ? (x.lyricsSynced ? t('synced') : t('plain')) + (x.lrcFile ? ' + .lrc' : '') : '—'],
    [t('fModified'), x.modified ? new Date(x.modified * 1000).toLocaleString(currentLang()) : '—'],
    ['MusicBrainz', x.tags.mbRecordingId || '—'],
  ];
  body.innerHTML = `<div class="kv">${kv.map(([k, v]) => `<div class="k">${esc(k)}</div><div class="v">${esc(v)}</div>`).join('')}</div>
    <div class="section-title" style="margin-top:20px">${icon('alert')}${esc(t('completeness'))}</div>
    <div class="row wrap">${x.missing.length ? x.missing.map((m) => `<span class="badge warn">${esc(t('mi_' + m))}</span>`).join('') : `<span class="badge ok">${esc(t('allComplete'))}</span>`}</div>`;
  foot.innerHTML = `${!isAndroid() ? `<button class="btn" id="f-reveal">${icon('folderOpen')}${esc(t('showInFolder'))}</button>` : ''}
    ${x.tags.mbRecordingId ? `<button class="btn" id="f-mb">${icon('external')}MusicBrainz</button>` : ''}
    <button class="btn" id="f-reload">${icon('refresh')}${esc(t('reload'))}</button>`;
  if ($('#f-reveal')) $('#f-reveal').onclick = () => opener?.revealItemInDir(x.path).catch((e) => toast(errMsg(e), 'bad'));
  if ($('#f-mb')) $('#f-mb').onclick = () => opener?.openUrl(`https://musicbrainz.org/recording/${x.tags.mbRecordingId}`);
  $('#f-reload').onclick = async () => { const nt = await invoke('reload_track', { id: x.id }); upsert(nt); S.covers.delete(x.id + ':300'); renderDrawer(); scheduleRender('rows'); };
}

// ───────────────────────────── Akcje ─────────────────────────────
async function chooseFolder() {
  let path = null;
  if (dialog && !isAndroid()) {
    try { path = await dialog.open({ directory: true, multiple: false, defaultPath: S.folder || undefined, title: t('chooseFolder') }); } catch (_) { path = null; }
    if (!path) return;
  } else {
    path = await folderBrowser(S.folder);
    if (!path) return;
  }
  scanFolder(path);
}
async function scanFolder(path) {
  if (S.job?.running) return toast(t('busy'), 'info');
  S.folder = path;
  S.scan = { done: 0, total: 0, phase: 'collect' };
  setTracks([]);
  closeDrawer(true);
  S.view = 'library';
  renderAll();
  try {
    const list = await invoke('scan_folder', { path, recursive: S.settings?.recursive ?? true });
    S.scan = null;
    setTracks(list);
    S.settings = await invoke('get_settings');
    renderAll();
    if (list.length) {
      const need = list.filter((x) => x.score < 90).length;
      toast(t('scanned', { n: list.length, need }), 'ok', need ? { label: t('smartFix'), run: () => analyzeIds(list.filter((x) => x.score < 100).map((x) => x.id)) } : null);
      loadDuplicates(true);
    }
  } catch (e) {
    S.scan = null;
    renderAll();
    toast(errMsg(e), 'bad');
  }
}
async function analyzeIds(ids) {
  if (!ids.length) return;
  try {
    await invoke('analyze_tracks', { ids });
    S.job = { kind: 'analyze', done: 0, total: ids.length, running: true };
    renderJobbar();
  } catch (e) { toast(errMsg(e), 'bad'); }
}
async function applyIds(ids) {
  ids = ids.filter((id) => S.proposals.has(id));
  if (!ids.length) return toast(t('nothingToApply'), 'info');
  try {
    await invoke('apply_proposals', { ids });
    S.job = { kind: 'apply', done: 0, total: ids.length, running: true };
    renderJobbar();
  } catch (e) { toast(errMsg(e), 'bad'); }
}
async function manualSearch(x) {
  const r = await modal({
    title: t('manualSearch'),
    body: `<p class="muted" style="margin-top:0">${esc(t('manualHint'))}</p><div class="form"><div class="field full"><label>${esc(t('f_title'))}</label><input class="inp" id="ms-t" value="${esc(x.tags.title || '')}"></div><div class="field full"><label>${esc(t('f_artist'))}</label><input class="inp" id="ms-a" value="${esc(x.tags.artist || '')}"></div></div>`,
    buttons: [{ label: t('cancel'), value: null }, { label: t('search'), cls: 'primary', icon: 'search', run: (ov) => ({ title: $('#ms-t', ov).value.trim(), artist: $('#ms-a', ov).value.trim() }) }],
  });
  if (!r || !r.title) return;
  toast(t('searching'), 'info');
  try {
    const p = await invoke('manual_search', { id: x.id, title: r.title, artist: r.artist || null });
    if (p) { S.proposals.set(x.id, p); openDrawer(x.id, 'proposal'); } else toast(t('notFoundMsg'), 'bad');
  } catch (e) { toast(errMsg(e), 'bad'); }
}
async function coverSearch(ids, query) {
  let results = [];
  const doSearch = async (ov, q) => {
    const grid = $('#cs-grid', ov);
    grid.innerHTML = `<div class="muted">${icon('refresh', 'spin')} ${esc(t('searching'))}</div>`;
    try {
      results = await invoke('search_covers', { query: q });
      grid.innerHTML = results.map((c, i) => `<div class="cpick" data-r="${i}" title="${esc(c.source)}"><img src="${esc(c.thumb)}" alt="" loading="lazy"><span class="src">${esc(c.source)}</span></div>`).join('');
    } catch (e) { grid.innerHTML = `<p class="muted">${esc(errMsg(e))}</p>`; }
  };
  let chosen = null;
  const ok = await modal({
    title: t('coverSearch'), wide: true,
    body: `<div class="row" style="margin-bottom:12px"><input class="inp grow" id="cs-q" value="${esc(query)}" placeholder="${esc(t('coverSearchPh'))}"><button class="btn" id="cs-go">${icon('search')}</button></div><div class="covers-pick" id="cs-grid"></div>`,
    buttons: [{ label: t('cancel'), value: false }, { label: t('useCover'), cls: 'primary', run: () => (chosen === null ? (toast(t('pickCover'), 'info'), false) : true) }],
    onMount: (ov) => {
      const q = $('#cs-q', ov);
      $('#cs-go', ov).onclick = () => doSearch(ov, q.value);
      q.onkeydown = (e) => { if (e.key === 'Enter') doSearch(ov, q.value); };
      $('#cs-grid', ov).onclick = (e) => { const el = e.target.closest('[data-r]'); if (!el) return; chosen = +el.dataset.r; $$('[data-r]', ov).forEach((x) => x.classList.toggle('on', x === el)); };
      if (query) doSearch(ov, query);
    },
  });
  if (!ok || chosen === null) return;
  try {
    const c = results[chosen];
    let b64;
    try { b64 = await invoke('fetch_cover_b64', { url: c.url }); } catch (_) { b64 = await invoke('fetch_cover_b64', { url: c.thumb }); }
    await setCover(ids, b64);
  } catch (e) { toast(errMsg(e), 'bad'); }
}
async function lyricsSearch(x, onPick) {
  let results = [];
  let picked = null;
  const run = async (ov, q) => {
    const box = $('#ls-list', ov);
    box.innerHTML = `<div class="muted">${icon('refresh', 'spin')} ${esc(t('searching'))}</div>`;
    try {
      results = await invoke('search_lyrics', { query: q });
      if (!results.length) { box.innerHTML = `<p class="muted">${esc(t('notFoundMsg'))}</p>`; return; }
      box.innerHTML = results.map((r, i) => {
        const dd = r.duration ? Math.abs(r.duration - x.duration) : null;
        return `<div class="result" data-i="${i}"><div class="grow"><b>${esc(r.trackName)}</b> — ${esc(r.artistName)}<div class="muted" style="font-size:12px">${esc(r.albumName || '')} · ${fmtDur(r.duration)}${dd !== null && dd > 3 ? ` <span class="badge warn">Δ ${Math.round(dd)} s</span>` : ''}</div></div>
          ${r.syncedLyrics ? `<span class="badge ok">${icon('clock')}${esc(t('synced'))}</span>` : r.plainLyrics ? `<span class="badge">${esc(t('plain'))}</span>` : `<span class="badge">${esc(t('instrumentalShort'))}</span>`}</div>`;
      }).join('');
    } catch (e) { box.innerHTML = `<p class="muted">${esc(errMsg(e))}</p>`; }
  };
  const q0 = [x.tags.artist, x.tags.title].filter(Boolean).join(' ') || x.fileName.replace(/\.[^.]+$/, '');
  await modal({
    title: t('lyricsSearch'), wide: true,
    body: `<div class="row" style="margin-bottom:12px"><input class="inp grow" id="ls-q" value="${esc(q0)}"><button class="btn" id="ls-go">${icon('search')}</button></div><div id="ls-list"></div><div id="ls-prev" class="lyrics-pre" style="display:none;margin-top:10px"></div>`,
    buttons: [{ label: t('cancel'), value: false }, { label: t('useLyrics'), cls: 'primary', run: () => (picked ? true : (toast(t('pickResult'), 'info'), false)) }],
    onMount: (ov) => {
      const q = $('#ls-q', ov);
      $('#ls-go', ov).onclick = () => run(ov, q.value);
      q.onkeydown = (e) => { if (e.key === 'Enter') run(ov, q.value); };
      $('#ls-list', ov).onclick = (e) => {
        const el = e.target.closest('[data-i]'); if (!el) return;
        const r = results[+el.dataset.i];
        picked = r.syncedLyrics || r.plainLyrics;
        $$('.result', ov).forEach((x) => x.style.borderColor = x === el ? 'var(--acc)' : '');
        const pv = $('#ls-prev', ov); pv.style.display = picked ? 'block' : 'none'; pv.textContent = picked || '';
      };
      run(ov, q0);
    },
  }).then((ok) => { if (ok && picked) onPick(picked); });
}
async function batchEdit(ids) {
  const rows = ['artist', 'albumArtist', 'album', 'year', 'genre', 'composer', 'label', 'discTotal', 'trackTotal'];
  const first = S.byId.get(ids[0]);
  const common = (f) => { const v = first?.tags[f]; return ids.every((id) => (S.byId.get(id)?.tags[f] ?? null) === (v ?? null)) ? v ?? '' : ''; };
  let coverData = null;
  const res = await modal({
    title: t('batchEditN', { n: ids.length }), wide: true,
    body: `<p class="muted" style="margin-top:0">${esc(t('batchHint'))}</p><div class="form">${rows.map((f) => `<div class="field"><label><button class="cb" data-on="${f}">${icon('check')}</button>${esc(fieldLabel(f))}</label><input class="inp" data-bf="${f}" value="${esc(common(f))}" placeholder="${esc(t('mixed'))}"></div>`).join('')}</div>
      <div class="section-title" style="margin-top:18px">${icon('sparkles')}${esc(t('batchTools'))}</div>
      <div class="row wrap"><button class="btn sm" id="b-num">${icon('list')}${esc(t('autoNumber'))}</button><button class="btn sm" id="b-cover">${icon('image')}${esc(t('coverSearch'))}</button>${isAndroid() ? '' : `<button class="btn sm" id="b-cfile">${icon('upload')}${esc(t('coverFile'))}</button>`}<span class="muted" id="b-cinfo"></span></div>`,
    buttons: [{ label: t('cancel'), value: null }, {
      label: t('saveAll'), cls: 'primary', run: (ov) => {
        const fields = {};
        $$('[data-on].on', ov).forEach((cb) => { fields[cb.dataset.on] = $(`[data-bf="${cb.dataset.on}"]`, ov).value.trim() || null; });
        return { fields, numbering: $('#b-num', ov).classList.contains('on') };
      },
    }],
    onMount: (ov) => {
      $$('[data-on]', ov).forEach((cb) => { cb.onclick = () => cb.classList.toggle('on'); });
      $$('[data-bf]', ov).forEach((inp) => { inp.oninput = () => $(`[data-on="${inp.dataset.bf}"]`, ov).classList.add('on'); });
      $('#b-num', ov).onclick = (e) => { e.currentTarget.classList.toggle('on'); e.currentTarget.classList.toggle('primary'); };
      $('#b-cover', ov).onclick = () => coverSearch(ids, [common('albumArtist') || common('artist'), common('album')].filter(Boolean).join(' '));
      if ($('#b-cfile', ov)) $('#b-cfile', ov).onclick = async () => {
        const path = await dialog?.open({ multiple: false, filters: [{ name: t('images'), extensions: ['jpg', 'jpeg', 'png', 'webp'] }] });
        if (!path) return;
        try { coverData = await invoke('read_image_file', { path }); $('#b-cinfo', ov).textContent = '✓ ' + baseName(path); } catch (e) { toast(errMsg(e), 'bad'); }
      };
    },
  });
  if (!res) return;
  try {
    if (Object.keys(res.fields).length || coverData)
      await invoke('save_edits', { req: { ids, fields: res.fields, cover: coverData ? { op: 'set', dataB64: coverData } : undefined } });
    if (res.numbering) {
      // Numeracja wg aktualnej kolejności na liście.
      const ordered = S.visible.filter((x) => ids.includes(x.id));
      for (let i = 0; i < ordered.length; i++)
        await invoke('save_edits', { req: { ids: [ordered[i].id], fields: { track: String(i + 1), trackTotal: String(ordered.length) } } });
    }
    toast(t('savedN', { n: ids.length }), 'ok');
  } catch (e) { toast(errMsg(e), 'bad'); }
}
async function renameDialog(ids) {
  const tokens = ['%artist%', '%title%', '%album%', '%track%', '%year%', '%albumartist%', '%disc%', '%genre%'];
  const presets = ['%artist% - %title%', '%track% - %title%', '%track%. %artist% - %title%', '%artist% - %album% - %track% - %title%'];
  const preview = async (ov) => {
    const pattern = $('#rn-p', ov).value;
    const list = await invoke('rename_preview', { ids, pattern });
    const changed = list.filter((r) => r.new && r.new !== r.old);
    $('#rn-sum', ov).textContent = t('renameSum', { n: changed.length, c: list.filter((r) => r.conflict).length });
    $('#rn-tbl', ov).innerHTML = `<table class="table"><tr><th>${esc(t('before'))}</th><th>${esc(t('after'))}</th></tr>${list.slice(0, 300).map((r) => `<tr><td class="dim">${esc(r.old)}</td><td class="${r.conflict ? 'bad' : ''}">${esc(r.new || t('noTitleSkip'))}</td></tr>`).join('')}</table>`;
  };
  const ok = await modal({
    title: t('renameN', { n: ids.length }), wide: true,
    body: `<div class="field"><label>${esc(t('pattern'))}</label><input class="inp mono" id="rn-p" value="${esc(S.settings?.renamePattern || presets[0])}"></div>
      <div class="row wrap" style="margin:8px 0">${tokens.map((tk) => `<button class="chip" data-tk="${tk}">${tk}</button>`).join('')}</div>
      <div class="row wrap" style="margin-bottom:10px">${presets.map((p) => `<button class="btn sm ghost" data-pr="${esc(p)}">${esc(p)}</button>`).join('')}</div>
      <div class="muted" id="rn-sum" style="margin-bottom:6px"></div><div id="rn-tbl" style="max-height:40vh;overflow:auto"></div>`,
    buttons: [{ label: t('cancel'), value: false }, { label: t('renameDo'), cls: 'primary', icon: 'rename', run: (ov) => $('#rn-p', ov).value }],
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
    toast(t('renamed', { n }), 'ok');
  } catch (e) { toast(errMsg(e), 'bad'); }
}
async function folderBrowser(start) {
  let cur = null;
  const load = async (ov, path) => {
    const list = $('#fb-list', ov), crumbs = $('#fb-crumbs', ov);
    try {
      if (!path) {
        const roots = await invoke('get_roots');
        cur = null;
        crumbs.innerHTML = `<span>${esc(t('locations'))}</span>`;
        list.innerHTML = roots.map((r) => `<div class="dir" data-p="${esc(r.path)}">${icon('home')}<span class="grow">${esc(r.name)}</span><span class="dim ellipsis" style="max-width:50%">${esc(r.path)}</span></div>`).join('') || `<p class="muted">${esc(t('noLocations'))}</p>`;
        $('#fb-info', ov).textContent = '';
        return;
      }
      const d = await invoke('list_dirs', { path });
      cur = d.path;
      const parts = d.path.split(/[\\/]/).filter(Boolean);
      const isWin = /^[A-Za-z]:/.test(d.path);
      crumbs.innerHTML = `<span data-p="">${icon('home')}</span>` + parts.map((p, i) => {
        const full = (isWin ? '' : '/') + parts.slice(0, i + 1).join(isWin ? '\\' : '/') + (isWin && i === 0 ? '\\' : '');
        return `/<span data-p="${esc(full)}">${esc(p)}</span>`;
      }).join('');
      list.innerHTML = (d.parent ? `<div class="dir" data-p="${esc(d.parent)}">${icon('arrowUp')}<span>..</span></div>` : '') +
        d.dirs.map((x) => `<div class="dir" data-p="${esc(x.path)}">${icon('folder')}<span class="grow ellipsis">${esc(x.name)}</span>${icon('chevronRight', 'dim')}</div>`).join('');
      $('#fb-info', ov).textContent = t('audioHere', { n: d.audioHere });
    } catch (e) { toast(errMsg(e), 'bad'); }
  };
  return modal({
    title: t('chooseFolder'),
    body: `<div class="crumbs" id="fb-crumbs"></div><div class="dir-list" id="fb-list"></div><div class="muted" id="fb-info" style="margin-top:10px;font-size:12.5px"></div>`,
    buttons: [{ label: t('cancel'), value: null }, { label: t('useThisFolder'), cls: 'primary', icon: 'check', run: () => cur || (toast(t('pickFolderFirst'), 'info'), false) }],
    onMount: (ov) => {
      ov.addEventListener('click', (e) => { const el = e.target.closest('[data-p]'); if (el && ov.contains(el)) load(ov, el.dataset.p || null); });
      load(ov, start || null);
    },
  });
}
async function exportReport() {
  const head = ['path', 'artist', 'title', 'album', 'albumArtist', 'year', 'track', 'genre', 'cover', 'lyrics', 'synced', 'score', 'missing'];
  const q = (v) => `"${String(v ?? '').replace(/"/g, '""')}"`;
  const rows = S.visible.map((x) => [x.path, x.tags.artist, x.tags.title, x.tags.album, x.tags.albumArtist, x.tags.year, x.tags.track, x.tags.genre,
    x.cover ? `${x.cover.width}x${x.cover.height}` : '', x.lyrics ? 'yes' : '', x.lyricsSynced ? 'yes' : '', x.score, x.missing.join(' ')].map(q).join(';'));
  const csv = [head.join(';'), ...rows].join('\r\n');
  let path = '';
  if (dialog && !isAndroid()) {
    path = await dialog.save({ defaultPath: 'musictagger-raport.csv', filters: [{ name: 'CSV', extensions: ['csv'] }] });
    if (!path) return;
  }
  try { const p = await invoke('write_text_file', { path, content: csv }); toast(t('reportSaved', { p }), 'ok'); } catch (e) { toast(errMsg(e), 'bad'); }
}
async function moreMenu() {
  const r = await modal({
    title: t('more'),
    body: `<div class="col" style="gap:6px">
      <button class="btn" data-m="selectNeed" style="justify-content:flex-start">${icon('filter')}${esc(t('selectIncomplete'))}</button>
      <button class="btn" data-m="selectAll" style="justify-content:flex-start">${icon('checkAll')}${esc(t('selectAllVisible'))}</button>
      <button class="btn" data-m="report" style="justify-content:flex-start">${icon('report')}${esc(t('exportReport'))}</button>
      <button class="btn" data-m="dups" style="justify-content:flex-start">${icon('duplicate')}${esc(t('nav_duplicates'))}</button>
      <button class="btn" data-m="rescan" style="justify-content:flex-start">${icon('refresh')}${esc(t('rescan'))}</button></div>`,
    onMount: (ov, done) => { $$('[data-m]', ov).forEach((b) => { b.onclick = () => done(b.dataset.m); }); },
  });
  if (r === 'selectNeed') { S.visible.filter((x) => x.score < 100).forEach((x) => S.selected.add(x.id)); paintRows(); renderSelbar(); }
  if (r === 'selectAll') { S.visible.forEach((x) => S.selected.add(x.id)); paintRows(); renderSelbar(); }
  if (r === 'report') exportReport();
  if (r === 'dups') go('duplicates');
  if (r === 'rescan' && S.folder) scanFolder(S.folder);
}

// ───────────────────────────── Przegląd propozycji ─────────────────────────────
function renderReview(v) {
  const list = S.tracks.filter((x) => x.status === 'proposed' && S.proposals.has(x.id));
  const th = S.settings?.autoApplyThreshold ?? 0.9;
  const sure = list.filter((x) => S.proposals.get(x.id).confidence >= th);
  for (const id of [...S.reviewSel]) if (!S.proposals.has(id)) S.reviewSel.delete(id);
  v.innerHTML = `<div class="page"><h1>${esc(t('nav_review'))}</h1><p>${esc(t('reviewText'))}</p>
    ${list.length ? `<div class="row wrap" style="margin-bottom:16px">
      <button class="btn primary" id="rv-sure" ${sure.length ? '' : 'disabled'}>${icon('checkAll')}${esc(t('applySure', { n: sure.length, p: Math.round(th * 100) }))}</button>
      <button class="btn" id="rv-sel" ${S.reviewSel.size ? '' : 'disabled'}>${icon('check')}${esc(t('applySelected', { n: S.reviewSel.size }))}</button>
      <button class="btn" id="rv-all">${icon('check')}${esc(t('applyAll', { n: list.length }))}</button>
      <button class="btn ghost danger" id="rv-discard">${icon('x')}${esc(t('discardAll'))}</button></div>` : `<div class="empty" style="padding:60px 0">${icon('sparkles', 'xl')}<p>${esc(t('reviewEmpty'))}</p>${S.tracks.length ? `<button class="btn primary" id="rv-start">${icon('sparkles')}${esc(t('smartFix'))}</button>` : ''}</div>`}
    ${list.map((x) => {
      const p = S.proposals.get(x.id);
      const pct = Math.round(p.confidence * 100);
      const enabled = p.changes.filter((c) => c.enabled);
      return `<div class="review-item" data-open-id="${x.id}"><div class="head"><button class="cb ${S.reviewSel.has(x.id) ? 'on' : ''}" data-rsel="${x.id}">${icon('check')}</button>${coverHTML(x)}
        <div class="grow"><div class="ellipsis"><b>${esc(x.tags.title || x.fileName)}</b></div><div class="muted ellipsis" style="font-size:12.5px">${esc(x.fileName)}</div></div>
        <span class="badge" style="color:${scoreColor(pct)}">${pct}%</span>
        <button class="btn sm" data-rapply="${x.id}">${icon('check')}</button></div>
        <div class="review-grid">${enabled.slice(0, 9).map((c) => `<div><div class="k">${esc(fieldLabel(c.field))}</div><div class="ellipsis" style="color:var(--ok)">${esc(c.new)}</div></div>`).join('')}
        ${p.coverEnabled ? `<div><div class="k">${esc(t('coverSec'))}</div><div style="color:var(--ok)">${icon('image')} ${esc(p.covers[p.coverChoice || 0]?.source || '')}</div></div>` : ''}
        ${p.lyricsEnabled ? `<div><div class="k">${esc(t('lyricsSec'))}</div><div style="color:var(--ok)">${icon('lyrics')} ${esc(p.lyrics?.synced ? t('synced') : t('plain'))}</div></div>` : ''}</div></div>`;
    }).join('')}</div>`;
  const ids = list.map((x) => x.id);
  $('#rv-sure') && ($('#rv-sure').onclick = () => applyIds(sure.map((x) => x.id)));
  $('#rv-all') && ($('#rv-all').onclick = () => applyIds(ids));
  $('#rv-sel') && ($('#rv-sel').onclick = () => applyIds([...S.reviewSel]));
  $('#rv-start') && ($('#rv-start').onclick = () => { go('library'); analyzeIds(S.tracks.filter((x) => x.score < 100).map((x) => x.id)); });
  $('#rv-discard') && ($('#rv-discard').onclick = async () => {
    if (!(await confirmBox(t('discardAll'), t('discardAllQ'), t('discard'), true))) return;
    await invoke('discard_proposals', { ids });
    ids.forEach((id) => { S.proposals.delete(id); const x = S.byId.get(id); if (x) x.status = 'idle'; });
    renderView(); renderNavBadges();
  });
  v.onclick = (e) => {
    const sel = e.target.closest('[data-rsel]');
    if (sel) { const id = sel.dataset.rsel; S.reviewSel.has(id) ? S.reviewSel.delete(id) : S.reviewSel.add(id); return renderView(); }
    const ap = e.target.closest('[data-rapply]');
    if (ap) return applyIds([ap.dataset.rapply]);
    const it = e.target.closest('[data-open-id]');
    if (it) openDrawer(it.dataset.openId, 'proposal');
  };
}

// ───────────────────────────── Duplikaty ─────────────────────────────
async function loadDuplicates(silent) {
  try { S.dups = await invoke('find_duplicates'); } catch (_) { S.dups = []; }
  renderNavBadges();
  if (!silent && S.view === 'duplicates') renderView();
}
function renderDuplicates(v) {
  const groups = (S.dups || []).map((g) => g.map((id) => S.byId.get(id)).filter(Boolean)).filter((g) => g.length > 1);
  v.innerHTML = `<div class="page"><h1>${esc(t('nav_duplicates'))}</h1><p>${esc(t('dupText'))}</p>
    ${!S.tracks.length ? `<p class="muted">${esc(t('scanFirst'))}</p>` : !groups.length ? `<div class="empty" style="padding:60px 0">${icon('shield', 'xl')}<p>${esc(t('noDups'))}</p></div>` : ''}
    ${groups.map((g) => {
      const best = g.slice().sort((a, b) => (b.bitrate || 0) - (a.bitrate || 0) || b.score - a.score)[0];
      return `<div class="dup-group"><div class="hd">${icon('duplicate')}<span class="grow ellipsis">${esc(g[0].tags.artist)} — ${esc(g[0].tags.title)}</span><span class="badge">${g.length}×</span></div>
      ${g.map((x) => `<div class="dup-row" data-open-id="${x.id}">${coverHTML(x)}<div class="grow" style="min-width:0"><div class="ellipsis">${esc(x.fileName)}</div><div class="muted ellipsis" style="font-size:12px">${esc(x.folder)}</div></div>
        <div class="col" style="gap:4px;align-items:flex-end"><span class="badge ${x === best ? 'ok' : ''}">${esc(x.format)} · ${x.bitrate || '?'} kbps</span><span class="dim" style="font-size:12px">${fmtDur(x.duration)} · ${fmtSize(x.size)}</span></div></div>`).join('')}</div>`;
    }).join('')}</div>`;
  v.onclick = (e) => { const it = e.target.closest('[data-open-id]'); if (it) openDrawer(it.dataset.openId, 'file'); };
}

// ───────────────────────────── Historia ─────────────────────────────
async function loadHistory() {
  try { S.history = await invoke('get_history'); } catch (_) { S.history = []; }
  if (S.view === 'history') renderView();
}
function renderHistory(v) {
  const h = S.history || [];
  const batches = [];
  const map = new Map();
  for (const e of h) {
    if (!map.has(e.batch)) { const b = { id: e.batch, time: e.time, items: [] }; map.set(e.batch, b); batches.push(b); }
    map.get(e.batch).items.push(e);
  }
  v.innerHTML = `<div class="page"><div class="row"><div class="grow"><h1>${esc(t('nav_history'))}</h1></div>${h.length ? `<button class="btn ghost danger" id="h-clear">${icon('trash')}${esc(t('clearHistory'))}</button>` : ''}</div>
    <p>${esc(t('historyText'))}</p>
    ${!h.length ? `<div class="empty" style="padding:60px 0">${icon('history', 'xl')}<p>${esc(t('historyEmpty'))}</p></div>` : ''}
    ${batches.slice(0, 200).map((b) => {
      const active = b.items.filter((x) => !x.undone).length;
      return `<div class="card" style="padding:0"><div class="hist-item" style="background:var(--panel2)">${icon('layers')}<div class="grow"><b>${esc(t('batchLabel', { n: b.items.length }))}</b><div class="muted" style="font-size:12px">${new Date(b.time * 1000).toLocaleString(currentLang())}</div></div>
        ${active ? `<button class="btn sm" data-undo="${esc(b.id)}">${icon('undo')}${esc(t('undoBatch'))}</button>` : `<span class="badge">${esc(t('undone'))}</span>`}</div>
        ${b.items.slice(0, 50).map((e) => `<div class="hist-item ${e.undone ? 'undone' : ''}">${icon(e.action === 'rename' ? 'rename' : e.action === 'edit' ? 'edit' : 'sparkles', 'dim')}
          <div class="grow" style="min-width:0"><div class="ellipsis">${esc(baseName(e.path))}</div><div class="muted ellipsis" style="font-size:12px">${esc(e.action === 'rename' ? e.summary.join(', ') : e.summary.map((s) => t('f_' + s) !== 'f_' + s ? t('f_' + s) : t('mi_' + s)).join(', '))}</div></div>
          ${e.undone ? '' : `<button class="btn sm ghost" data-undo="${esc(e.id)}" title="${esc(t('undo'))}">${icon('undo')}</button>`}</div>`).join('')}
        ${b.items.length > 50 ? `<div class="hist-item muted">+${b.items.length - 50}</div>` : ''}</div>`;
    }).join('')}</div>`;
  v.onclick = async (e) => {
    const u = e.target.closest('[data-undo]');
    if (u) {
      try { const n = await invoke('undo', { ids: [u.dataset.undo] }); toast(t('undoneN', { n }), 'ok'); loadHistory(); } catch (err) { toast(errMsg(err), 'bad'); }
    }
    if (e.target.closest('#h-clear')) {
      if (await confirmBox(t('clearHistory'), t('clearHistoryQ'), t('remove'), true)) { await invoke('clear_history'); loadHistory(); }
    }
  };
}
async function undoLast() {
  const h = await invoke('get_history');
  const last = h.find((e) => !e.undone);
  if (!last) return toast(t('historyEmpty'), 'info');
  try { const n = await invoke('undo', { ids: [last.batch] }); toast(t('undoneN', { n }), 'ok'); } catch (e) { toast(errMsg(e), 'bad'); }
}

// ───────────────────────────── Ustawienia ─────────────────────────────
function renderSettings(v) {
  const s = S.settings;
  const sw = (k) => `<button class="switch ${s[k] ? 'on' : ''}" data-sw="${k}" role="switch" aria-checked="${!!s[k]}"></button>`;
  const set = (k, title, desc, ctrl) => `<div class="setting"><div class="txt"><b>${esc(title)}</b>${desc ? `<span>${desc}</span>` : ''}</div>${ctrl}</div>`;
  const num = (k, min, max, step, suffix = '') => `<div class="range-wrap"><input type="range" data-num="${k}" min="${min}" max="${max}" step="${step}" value="${s[k]}"><b data-numv="${k}">${s[k]}${suffix}</b></div>`;
  const sel = (k, opts) => `<select class="sel" data-sel="${k}">${opts.map(([val, l]) => `<option value="${val}" ${String(s[k]) === String(val) ? 'selected' : ''}>${esc(l)}</option>`).join('')}</select>`;
  v.innerHTML = `<div class="page" style="max-width:900px"><h1>${esc(t('nav_settings'))}</h1><p>${esc(t('settingsText'))}</p>
    <div class="card"><h2>${icon('sun')}${esc(t('sAppearance'))}</h2>
      ${set('theme', t('theme'), '', sel('theme', [['dark', t('themeDark')], ['light', t('themeLight')], ['auto', t('themeAuto')]]))}
      ${set('accent', t('accent'), '', `<div class="swatches">${['violet', 'blue', 'green', 'orange', 'pink'].map((a) => `<button class="swatch ${s.accent === a ? 'on' : ''}" data-accent-pick="${a}" style="background:${{ violet: '#8b5cf6', blue: '#3b82f6', green: '#10b981', orange: '#f97316', pink: '#ec4899' }[a]}"></button>`).join('')}</div>`)}
      ${set('language', t('language'), '', sel('language', [['auto', t('langAuto')], ['pl', 'Polski'], ['en', 'English']]))}
    </div>
    <div class="card"><h2>${icon('sparkles')}${esc(t('sBehavior'))}</h2>
      ${set('writeMode', t('writeMode'), esc(t('writeModeD')), sel('writeMode', [['missing', t('wmMissing')], ['overwrite', t('wmOverwrite')]]))}
      ${set('autoApply', t('autoApply'), esc(t('autoApplyD')), sw('autoApply'))}
      ${set('autoApplyThreshold', t('threshold'), esc(t('thresholdD')), `<div class="range-wrap"><input type="range" data-num="autoApplyThreshold" min="0.6" max="1" step="0.01" value="${s.autoApplyThreshold}"><b data-numv="autoApplyThreshold">${Math.round(s.autoApplyThreshold * 100)}%</b></div>`)}
      ${set('preferOriginalRelease', t('preferOriginal'), esc(t('preferOriginalD')), sw('preferOriginalRelease'))}
      ${set('fillAlbumArtist', t('fillAlbumArtist'), '', sw('fillAlbumArtist'))}
      ${set('fillGenre', t('fillGenre'), '', sw('fillGenre'))}
      ${set('writeMbIds', t('writeMbIds'), esc(t('writeMbIdsD')), sw('writeMbIds'))}
      ${set('recursive', t('recursive'), '', sw('recursive'))}
      ${set('concurrency', t('concurrency'), esc(t('concurrencyD')), num('concurrency', 1, 8, 1))}
    </div>
    <div class="card"><h2>${icon('image')}${esc(t('sCovers'))}</h2>
      ${set('fetchCovers', t('fetchCovers'), '', sw('fetchCovers'))}
      ${set('replaceSmallCovers', t('replaceSmall'), esc(t('replaceSmallD')), sw('replaceSmallCovers'))}
      ${set('minCoverSize', t('minCover'), '', num('minCoverSize', 200, 1000, 50, ' px'))}
      ${set('maxCoverSize', t('maxCover'), esc(t('maxCoverD')), num('maxCoverSize', 500, 3000, 100, ' px'))}
      ${set('coverQuality', t('coverQuality'), '', num('coverQuality', 60, 100, 1, '%'))}
      ${set('saveFolderJpg', t('saveFolderJpg'), esc(t('saveFolderJpgD')), sw('saveFolderJpg'))}
    </div>
    <div class="card"><h2>${icon('lyrics')}${esc(t('sLyrics'))}</h2>
      ${set('fetchLyrics', t('fetchLyrics'), '', sw('fetchLyrics'))}
      ${set('preferSynced', t('preferSynced'), esc(t('preferSyncedD')), sw('preferSynced'))}
      ${set('embedLyrics', t('embedLyrics'), esc(t('embedLyricsD')), sw('embedLyrics'))}
      ${set('writeLrcFile', t('writeLrc'), esc(t('writeLrcD')), sw('writeLrcFile'))}
    </div>
    <div class="card"><h2>${icon('layers')}${esc(t('sSources'))}</h2>
      ${set('useMusicbrainz', 'MusicBrainz + Cover Art Archive', esc(t('srcMb')), sw('useMusicbrainz'))}
      ${set('useDeezer', 'Deezer', esc(t('srcDeezer')), sw('useDeezer'))}
      ${set('useItunes', 'Apple iTunes', esc(t('srcItunes')), sw('useItunes'))}
      ${set('itunesCountry', t('itunesCountry'), '', sel('itunesCountry', [['PL', 'Polska'], ['US', 'USA'], ['GB', 'UK'], ['DE', 'Deutschland'], ['FR', 'France'], ['JP', 'Japan']]))}
      ${set('useLrclib', 'LRCLIB', esc(t('srcLrclib')), sw('useLrclib'))}
      ${set('useAcoustid', 'AcoustID', esc(t('srcAcoustid')) + ` <a href="#" id="acoustid-link">${esc(t('getKey'))}</a>`, sw('useAcoustid'))}
      ${set('acoustidKey', t('acoustidKey'), esc(t('acoustidKeyD')), `<input class="inp mono" data-text="acoustidKey" value="${esc(s.acoustidKey)}" placeholder="np. AbCdEf123">`)}
    </div>
    <div class="card"><h2>${icon('rename')}${esc(t('sRename'))}</h2>
      ${set('renamePattern', t('pattern'), esc(t('patternD')), `<input class="inp mono" data-text="renamePattern" value="${esc(s.renamePattern)}">`)}
    </div>
    <div class="card"><h2>${icon('info')}${esc(t('sAbout'))}</h2>
      ${set('', 'MusicTagger ' + S.info.version, esc(t('aboutD')), `<button class="btn sm" id="chk-upd">${icon('update')}${esc(t('checkUpdates'))}</button>`)}
      ${set('', t('shortcuts'), esc(t('shortcutsD')), '')}
      ${set('', t('dataDir'), `<span class="mono selectable">${esc(S.info.dataDir || '')}</span>`, '')}
    </div></div>`;
  const persist = debounce(async () => {
    try { S.settings = await invoke('save_settings', { settings: S.settings }); } catch (e) { toast(errMsg(e), 'bad'); }
  }, 300);
  $$('[data-sw]', v).forEach((b) => { b.onclick = () => { const k = b.dataset.sw; s[k] = !s[k]; b.classList.toggle('on', s[k]); persist(); }; });
  $$('[data-num]', v).forEach((r) => {
    r.oninput = () => {
      const k = r.dataset.num; s[k] = k === 'autoApplyThreshold' ? +r.value : Math.round(+r.value);
      $(`[data-numv="${k}"]`, v).textContent = k === 'autoApplyThreshold' ? Math.round(s[k] * 100) + '%' : s[k] + (k.includes('Size') ? ' px' : k === 'coverQuality' ? '%' : '');
      persist();
    };
  });
  $$('[data-sel]', v).forEach((el) => {
    el.onchange = () => {
      const k = el.dataset.sel; s[k] = el.value; persist();
      if (k === 'theme') applyTheme();
      if (k === 'language') { setLang(s.language); renderAll(); }
    };
  });
  $$('[data-text]', v).forEach((el) => { el.oninput = () => { s[el.dataset.text] = el.value.trim(); persist(); }; });
  $$('[data-accent-pick]', v).forEach((b) => { b.onclick = () => { s.accent = b.dataset.accentPick; applyTheme(); persist(); renderView(); }; });
  $('#acoustid-link').onclick = (e) => { e.preventDefault(); opener?.openUrl('https://acoustid.org/new-application'); };
  $('#chk-upd').onclick = () => checkUpdate(false);
}
function applyTheme() {
  const s = S.settings || {};
  let th = s.theme || 'dark';
  if (th === 'auto') th = window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
  document.documentElement.dataset.theme = th;
  document.documentElement.dataset.accent = s.accent || 'violet';
}
async function checkUpdate(silent) {
  try {
    const u = await invoke('check_update');
    if (u.available) toast(t('updateAvail', { v: u.latest }), 'info', { label: t('download'), run: () => opener?.openUrl(u.download || u.url) });
    else if (!silent) toast(t('upToDate'), 'ok');
  } catch (e) { if (!silent) toast(errMsg(e), 'bad'); }
}

// ───────────────────────────── Zdarzenia globalne ─────────────────────────────
document.addEventListener('click', async (e) => {
  const el = e.target.closest('button, [data-goto], [data-album], [data-filter]');
  if (!el) return;
  if (el.dataset.nav) return go(el.dataset.nav);
  if (el.dataset.open) return scanFolder(el.dataset.open);
  if (el.dataset.filter) { S.filter = el.dataset.filter; return renderView(); }
  if (el.dataset.goto) { S.filter = el.dataset.goto; S.mode === 'albums' && (S.mode = 'list'); return renderView(); }
  if (el.dataset.album) { S.album = el.dataset.album; return renderView(); }
  if (el.dataset.mode) { S.mode = el.dataset.mode; localStore('mode', S.mode); return renderView(); }
  if (el.dataset.tab) { S.tab = el.dataset.tab; stopAudio(); return renderDrawer(); }
  if (el.dataset.sort) { S.sort = { key: el.dataset.sort, dir: S.sort.key === el.dataset.sort ? -S.sort.dir : 1 }; return renderView(); }
  if (el.hasAttribute('data-theme-toggle')) { S.settings.theme = S.settings.theme === 'light' ? 'dark' : 'light'; applyTheme(); renderSidebar(); invoke('save_settings', { settings: S.settings }); return; }
  if (el.dataset.act) {
    const ids = [...S.selected];
    if (el.dataset.act === 'analyze') analyzeIds(ids);
    if (el.dataset.act === 'apply') applyIds(ids);
    if (el.dataset.act === 'batch') batchEdit(ids);
    if (el.dataset.act === 'rename') renameDialog(ids);
    if (el.dataset.act === 'clear') { S.selected.clear(); paintRows(); renderSelbar(); }
    return;
  }
  switch (el.id) {
    case 'pick-folder': case 'pick-folder2': return chooseFolder();
    case 'rescan': return S.folder && scanFolder(S.folder);
    case 'smart-all': {
      const ids = (S.selected.size ? [...S.selected] : S.visible.map((x) => x.id)).filter((id) => (S.byId.get(id)?.score ?? 0) < 100 || S.selected.size);
      if (!ids.length) return toast(t('allComplete'), 'ok');
      return analyzeIds(ids);
    }
    case 'cancel-job': return invoke('cancel_job');
    case 'sel-all': {
      const all = S.visible.every((x) => S.selected.has(x.id));
      S.visible.forEach((x) => (all ? S.selected.delete(x.id) : S.selected.add(x.id)));
      paintRows(); return renderSelbar();
    }
    case 'clear-album': S.album = null; return renderView();
    case 'sort-dir': S.sort.dir = -S.sort.dir; return renderView();
    case 'more-actions': return moreMenu();
    case 'd-close': return closeDrawer();
    case 'd-prev': return stepActive(-1);
    case 'd-next': return stepActive(1);
    case 'perm-check': S.storageOk = await invoke('storage_access'); return renderView();
  }
});
document.addEventListener('change', (e) => {
  if (e.target.id === 'sort') { S.sort = { key: e.target.value, dir: 1 }; renderView(); }
});
const onSearch = debounce(() => { computeVisible(); const w = $('#listwrap'); if (!w) return; if (S.mode === 'albums' && !S.album) renderAlbums(w); else renderList(w); }, 120);
document.addEventListener('input', (e) => { if (e.target.id === 'q') { S.query = e.target.value; onSearch(); } });
document.addEventListener('keydown', (e) => {
  const inField = e.target.matches('input, textarea, [contenteditable="true"], select');
  const mod = e.ctrlKey || e.metaKey;
  if (e.key === 'Escape') {
    if (modalStack.length) return modalStack[modalStack.length - 1].close();
    if (S.active) return closeDrawer();
    if (S.selected.size) { S.selected.clear(); paintRows(); return renderSelbar(); }
  }
  if (mod && e.key.toLowerCase() === 'f') { e.preventDefault(); if (S.view !== 'library') go('library'); setTimeout(() => $('#q')?.focus(), 0); return; }
  if (mod && e.key.toLowerCase() === 'o') { e.preventDefault(); return chooseFolder(); }
  if (inField || modalStack.length) return;
  if (mod && e.key.toLowerCase() === 'a' && S.view === 'library') { e.preventDefault(); S.visible.forEach((x) => S.selected.add(x.id)); paintRows(); return renderSelbar(); }
  if (mod && e.key.toLowerCase() === 'z') { e.preventDefault(); return undoLast(); }
  if (S.active && (e.key === 'ArrowDown' || e.key === 'ArrowUp')) { e.preventDefault(); return stepActive(e.key === 'ArrowDown' ? 1 : -1); }
  if (S.active && e.key === 'Enter' && S.proposals.has(S.active)) return applyIds([S.active]);
  if (e.key === ' ' && S.active && S.tab === 'lyrics') { e.preventDefault(); $('#l-play')?.click(); }
});
window.addEventListener('resize', debounce(() => { if (S.view === 'library') paintRows(); }, 100));
window.matchMedia('(prefers-color-scheme: light)').addEventListener?.('change', applyTheme);

// ───────────────────────────── Start ─────────────────────────────
async function init() {
  try {
    S.settings = await invoke('get_settings');
    S.info = await invoke('app_info');
  } catch (e) {
    S.settings = S.settings || {};
    console.error(e);
  }
  setLang(S.settings.language || 'auto');
  applyTheme();
  if (isAndroid()) S.storageOk = await invoke('storage_access').catch(() => true);
  renderAll();

  listen('scan-progress', (e) => { S.scan = e.payload; renderJobbar(); });
  listen('track-update', (e) => {
    const { track, proposal } = e.payload;
    upsert(track, proposal);
    scheduleRender('rows');
    if (S.active === track.id) scheduleRender('drawer');
  });
  listen('track-replaced', (e) => {
    replaceTrack(e.payload.oldId, e.payload.track);
    scheduleRender('rows');
    if (S.active === e.payload.track.id) scheduleRender('drawer');
  });
  listen('job-progress', (e) => {
    S.job = e.payload;
    renderJobbar();
    if (!e.payload.running) {
      const j = e.payload;
      if (j.kind === 'analyze') {
        const proposed = S.tracks.filter((x) => x.status === 'proposed').length;
        const applied = S.tracks.filter((x) => x.status === 'applied').length;
        const nf = S.tracks.filter((x) => x.status === 'notFound').length;
        toast(t('analyzeDone', { p: proposed, a: applied, nf }), 'ok', proposed ? { label: t('review'), run: () => go('review') } : null);
      } else toast(t('applyDone', { n: j.done - j.errors, e: j.errors }), j.errors ? 'bad' : 'ok', { label: t('undo'), run: undoLast });
      if (S.view === 'review') renderView();
      renderNavBadges();
      loadDuplicates(true);
    }
  });
  // Przeciąganie folderu na okno.
  listen('tauri://drag-enter', () => { $('#drop-overlay').hidden = false; $('#drop-overlay').textContent = t('dropHere'); });
  listen('tauri://drag-leave', () => { $('#drop-overlay').hidden = true; });
  listen('tauri://drag-drop', (e) => {
    $('#drop-overlay').hidden = true;
    const p = e.payload?.paths?.[0];
    if (p) {
      const isFile = /\.[a-z0-9]{2,4}$/i.test(p);
      scanFolder(isFile ? p.replace(/[\\/][^\\/]+$/, '') : p);
    }
  });

  const last = localStore('updChecked');
  if (!last || Date.now() - +last > 86400000) { localStore('updChecked', String(Date.now())); setTimeout(() => checkUpdate(true), 4000); }
}
init();
