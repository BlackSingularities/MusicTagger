// Zgłoszenia do redzikowski.com (wspólny moduł aplikacji z interfejsem webowym: Tauri, Electron):
//  - raport o błędzie – nieobsłużony wyjątek zapamiętany w localStorage i zaproponowany
//    do wysłania przy następnym starcie (trafia do „Raportów błędów” w panelu),
//  - zgłoszenie błędu albo sugestia funkcjonalności pisane ręcznie (trafia do
//    „Sugestii i zgłoszeń”).
// Adres e-mail jest zawsze opcjonalny; nic nie wychodzi bez akcji użytkownika.
// Bez zależności – okno i style tworzy sam moduł. Plik jest taki sam we wszystkich
// aplikacjach webowych – zmieniaj go razem we wszystkich.

const ENDPOINT = "https://redzikowski.com/api/feedback";
const PENDING_KEY = "redzikowski-feedback:pending-crash";

const TEXT = {
  pl: {
    title: "Zgłoś błąd lub sugestię",
    bug: "Błąd",
    feature: "Sugestia funkcji",
    bugPrompt: "Co się stało? Co robiłeś(-aś) tuż przed błędem?",
    featurePrompt: "Czego brakuje albo co można zrobić lepiej?",
    email: "Twój e-mail (opcjonalnie – jeśli chcesz odpowiedź)",
    emailPlaceholder: "np. jan@example.com",
    emailBad: "To nie wygląda na adres e-mail.",
    device: "Dołącz wersję aplikacji i systemu",
    send: "Wyślij",
    sending: "Wysyłanie…",
    cancel: "Anuluj",
    close: "Zamknij",
    footer: "Zgłoszenie trafi do autora przez redzikowski.com.",
    thanks: "Dziękuję! Zgłoszenie dotarło.",
    thanksNoMail: "Przeczytam je i wezmę pod uwagę przy kolejnych wersjach.",
    thanksMail: "Przeczytam je i odezwę się na podany adres, jeśli będzie trzeba.",
    crashTitle: "Wystąpił błąd aplikacji",
    crashText: "Przy ostatnim użyciu wystąpił nieoczekiwany błąd. Czy wysłać raport, żeby można go było naprawić? Raport zawiera opis błędu, wersję aplikacji i systemu – żadnych Twoich plików.",
    crashDetails: "Pokaż szczegóły",
    crashComment: "Co robiłeś(-aś) przed błędem? (opcjonalnie)",
    crashSend: "Wyślij raport",
    crashSkip: "Nie wysyłaj",
    crashThanks: "Dziękuję! Raport został wysłany.",
    tooMany: "Za dużo zgłoszeń – spróbuj później.",
    rejected: "Serwer odrzucił zgłoszenie",
    offline: "Brak połączenia z redzikowski.com.",
  },
  en: {
    title: "Report a problem or suggest a feature",
    bug: "Bug",
    feature: "Feature idea",
    bugPrompt: "What happened? What were you doing right before?",
    featurePrompt: "What's missing or could work better?",
    email: "Your e-mail (optional – if you'd like a reply)",
    emailPlaceholder: "e.g. jane@example.com",
    emailBad: "That doesn't look like an e-mail address.",
    device: "Include app and system version",
    send: "Send",
    sending: "Sending…",
    cancel: "Cancel",
    close: "Close",
    footer: "Goes to the author via redzikowski.com.",
    thanks: "Thank you! Your message has arrived.",
    thanksNoMail: "I'll read it and take it into account in future versions.",
    thanksMail: "I'll read it and reply to the address you gave if needed.",
    crashTitle: "The app ran into an error",
    crashText: "An unexpected error occurred last time. Send a report so it can be fixed? It contains the error description and the app and system version – none of your files.",
    crashDetails: "Show details",
    crashComment: "What were you doing before the error? (optional)",
    crashSend: "Send report",
    crashSkip: "Don't send",
    crashThanks: "Thank you! The report has been sent.",
    tooMany: "Too many reports – try again later.",
    rejected: "The server rejected the report",
    offline: "Can't reach redzikowski.com.",
  },
};

/** @type {{ app: string, appName: string, version: string | (() => string | Promise<string>), lang?: string | (() => string), platform?: string }} */
let config = { app: "", appName: "", version: "" };
let installed = false;

function lang() {
  const l = typeof config.lang === "function" ? config.lang() : config.lang || navigator.language || "en";
  return String(l).toLowerCase().startsWith("pl") ? "pl" : "en";
}
function t(key) {
  return TEXT[lang()][key];
}

function platform() {
  if (config.platform) return config.platform;
  const ua = navigator.userAgent;
  if (/android/i.test(ua)) return "android";
  if (/iphone|ipad|ipod/i.test(ua) || (/macintosh/i.test(ua) && navigator.maxTouchPoints > 1)) return "ios";
  if (/windows/i.test(ua)) return "windows";
  if (/mac os/i.test(ua)) return "macos";
  if (/linux/i.test(ua)) return "linux";
  return "other";
}

function osName() {
  const ua = navigator.userAgent;
  const m =
    ua.match(/Windows NT [\d.]+/) || ua.match(/Android [\d.]+/) || ua.match(/(?:iPhone )?OS [\d_]+/) || ua.match(/Mac OS X [\d_]+/);
  return m ? m[0].replace(/_/g, ".") : navigator.platform || "";
}

async function version() {
  try {
    return String((typeof config.version === "function" ? await config.version() : config.version) || "");
  } catch {
    return "";
  }
}

function emailOk(e) {
  const v = e.trim();
  return !v || /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(v);
}

async function post(body) {
  let r;
  try {
    r = await fetch(ENDPOINT, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
  } catch {
    throw new Error(t("offline"));
  }
  if (r.status === 429) throw new Error(t("tooMany"));
  if (!r.ok) throw new Error(`${t("rejected")} (HTTP ${r.status}).`);
}

async function base(includeDevice) {
  const b = {
    app: config.app,
    appName: config.appName,
    appVersion: await version(),
    platform: platform(),
    locale: navigator.language || "",
  };
  if (includeDevice) {
    b.os = osName();
    b.device = "";
    b.extra = {
      userAgent: navigator.userAgent,
      screen: `${screen.width}×${screen.height} @${window.devicePixelRatio || 1}x`,
      cpus: navigator.hardwareConcurrency || 0,
    };
  }
  return b;
}

/* ── awarie ─────────────────────────────────────────────────────────── */

function remember(kind, error) {
  try {
    const stack =
      (error && (error.stack || error.message)) || (typeof error === "string" ? error : JSON.stringify(error)) || String(error);
    const text = `${kind}: ${String(stack)}`.slice(0, 60000);
    // Pierwszy błąd sesji jest zwykle przyczyną – kolejne tylko go powtarzają.
    if (localStorage.getItem(PENDING_KEY)) return;
    localStorage.setItem(PENDING_KEY, JSON.stringify({ stack: text, at: Date.now(), url: location.pathname }));
  } catch {
    /* localStorage niedostępny – trudno */
  }
}

function pending() {
  try {
    const raw = localStorage.getItem(PENDING_KEY);
    return raw ? JSON.parse(raw) : null;
  } catch {
    return null;
  }
}
function discard() {
  try {
    localStorage.removeItem(PENDING_KEY);
  } catch {
    /* nic */
  }
}

/**
 * Wywołaj raz przy starcie aplikacji.
 * `offerCrash` = false, gdy aplikacja chce sama zdecydować, kiedy pokazać raport ({@link offerPendingCrash}).
 */
export function initFeedback(options) {
  config = { ...config, ...options };
  if (installed) return;
  installed = true;
  window.addEventListener("error", (e) => {
    // Błędy ładowania obrazków/skryptów nie są awariami aplikacji.
    if (!e.error && !e.message) return;
    remember("Error", e.error || `${e.message} (${e.filename}:${e.lineno}:${e.colno})`);
  });
  window.addEventListener("unhandledrejection", (e) => {
    const reason = e.reason;
    // Przerwane żądania sieci to nie błąd programu.
    if (reason && (reason.name === "AbortError" || /Failed to fetch|NetworkError|Load failed/i.test(String(reason.message || reason)))) return;
    remember("Unhandled rejection", reason);
  });
  if (options.offerCrash !== false) setTimeout(offerPendingCrash, 1500);
}

/** Zapis błędu z własnej obsługi wyjątków aplikacji (np. try/catch na najwyższym poziomie). */
export function reportError(error) {
  remember("Error", error);
}

/* ── okno ───────────────────────────────────────────────────────────── */

const STYLE = `
.rfb-backdrop{position:fixed;inset:0;z-index:2147483000;display:flex;align-items:center;justify-content:center;padding:16px;background:rgba(0,0,0,.45);
  --rfb-bg:#ffffff;--rfb-fg:#17161a;--rfb-dim:#6b6874;--rfb-line:#d9d6de;--rfb-field:#f6f5f8;--rfb-accent:#2f6fde;--rfb-accent-fg:#fff;--rfb-err:#c4323a;
  font:14px/1.45 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;color:var(--rfb-fg)}
@media (prefers-color-scheme:dark){.rfb-backdrop{--rfb-bg:#1e1d22;--rfb-fg:#f1eff4;--rfb-dim:#a19dab;--rfb-line:#3a3842;--rfb-field:#26252b;--rfb-accent:#6b9cff;--rfb-accent-fg:#0d0d10;--rfb-err:#ff6b72}}
.rfb-box{box-sizing:border-box;width:min(500px,100%);min-width:0;max-height:calc(100vh - 32px);overflow:auto;background:var(--rfb-bg);border:1px solid var(--rfb-line);border-radius:14px;padding:20px;box-shadow:0 18px 50px rgba(0,0,0,.35);
  padding-bottom:max(20px,env(safe-area-inset-bottom))}
.rfb-box *{box-sizing:border-box}
.rfb-box h2{margin:0 0 12px;font-size:18px;font-weight:650}
.rfb-box p{margin:0 0 10px;color:var(--rfb-dim)}
.rfb-box label{display:block;margin:12px 0 6px;font-weight:550}
.rfb-box textarea,.rfb-box input[type=email]{width:100%;font:inherit;color:inherit;background:var(--rfb-field);border:1px solid var(--rfb-line);border-radius:9px;padding:9px 11px;outline:none}
.rfb-box textarea{min-height:120px;resize:vertical}
.rfb-box textarea:focus,.rfb-box input:focus{border-color:var(--rfb-accent)}
.rfb-seg{display:flex;gap:6px;background:var(--rfb-field);border:1px solid var(--rfb-line);border-radius:10px;padding:3px;width:max-content;max-width:100%}
.rfb-seg button{border:0;background:transparent;color:var(--rfb-dim);font:inherit;padding:6px 14px;border-radius:7px;cursor:pointer}
.rfb-seg button[aria-pressed=true]{background:var(--rfb-bg);color:var(--rfb-fg);box-shadow:0 1px 3px rgba(0,0,0,.18)}
.rfb-check{display:flex!important;gap:8px;align-items:center;font-weight:400!important;color:var(--rfb-dim)}
.rfb-err{color:var(--rfb-err);margin-top:8px;min-height:0}
.rfb-row{display:flex;gap:8px;justify-content:flex-end;flex-wrap:wrap;margin-top:16px}
.rfb-row button{font:inherit;border-radius:9px;padding:8px 16px;cursor:pointer;border:1px solid var(--rfb-line);background:transparent;color:inherit}
.rfb-row button.rfb-primary{background:var(--rfb-accent);border-color:var(--rfb-accent);color:var(--rfb-accent-fg);font-weight:600}
.rfb-row button:disabled{opacity:.5;cursor:default}
.rfb-foot{margin-top:12px;font-size:12px;color:var(--rfb-dim)}
.rfb-box details{margin:4px 0 6px;min-width:0}
.rfb-box summary{cursor:pointer;color:var(--rfb-dim)}
.rfb-box pre{max-width:100%;max-height:180px;overflow:auto;font:11px/1.4 ui-monospace,Consolas,monospace;background:var(--rfb-field);border:1px solid var(--rfb-line);border-radius:9px;padding:8px;white-space:pre}
`;

let open = null;

/** Otwarte okno – także gdy aplikacja sama wyrzuciła je z DOM, uznajemy je za zamknięte. */
function isOpen() {
  if (open && !open.backdrop.isConnected) open = null;
  return !!open;
}

function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (k === "class") node.className = v;
    else if (k.startsWith("on")) node.addEventListener(k.slice(2), v);
    else if (v === true) node.setAttribute(k, "");
    else if (v !== false && v != null) node.setAttribute(k, v);
  }
  for (const c of children) if (c != null) node.append(c);
  return node;
}

function mount(build) {
  if (isOpen()) open.close();
  if (!document.getElementById("rfb-style")) document.head.append(el("style", { id: "rfb-style" }, STYLE));
  const box = el("div", { class: "rfb-box", role: "dialog", "aria-modal": "true" });
  const backdrop = el("div", { class: "rfb-backdrop" }, box);
  let busy = false;
  const api = {
    box,
    backdrop,
    setBusy(v) {
      busy = v;
    },
    close() {
      backdrop.remove();
      document.removeEventListener("keydown", onKey, true);
      open = null;
    },
  };
  function onKey(e) {
    if (e.key === "Escape" && !busy) {
      e.stopPropagation();
      api.close();
    }
  }
  document.addEventListener("keydown", onKey, true);
  backdrop.addEventListener("mousedown", (e) => {
    if (e.target === backdrop && !busy) api.close();
  });
  build(api);
  document.body.append(backdrop);
  open = api;
  setTimeout(() => box.querySelector("textarea")?.focus(), 30);
}

function thanks(api, title, withMail) {
  api.box.replaceChildren(
    el("h2", {}, title),
    el("p", {}, withMail ? t("thanksMail") : t("thanksNoMail")),
    el("div", { class: "rfb-row" }, el("button", { class: "rfb-primary", onclick: () => api.close() }, t("close"))),
  );
}

function emailBlock() {
  const input = el("input", { type: "email", placeholder: t("emailPlaceholder"), autocomplete: "email", inputmode: "email" });
  return { input, nodes: [el("label", {}, t("email")), input] };
}

/** Formularz „Zgłoś błąd lub sugestię”. */
export function openFeedback(kind = "bug") {
  mount((api) => {
    let feature = kind === "feature";
    const bugBtn = el("button", { type: "button" }, t("bug"));
    const featBtn = el("button", { type: "button" }, t("feature"));
    const prompt = el("label", {});
    const message = el("textarea", { maxlength: "8000" });
    const email = emailBlock();
    const device = el("input", { type: "checkbox", checked: true });
    const error = el("div", { class: "rfb-err", role: "alert" });
    const send = el("button", { class: "rfb-primary", type: "button" }, t("send"));
    const cancel = el("button", { type: "button", onclick: () => api.close() }, t("cancel"));
    const sync = () => {
      bugBtn.setAttribute("aria-pressed", String(!feature));
      featBtn.setAttribute("aria-pressed", String(feature));
      prompt.textContent = feature ? t("featurePrompt") : t("bugPrompt");
      send.disabled = message.value.trim().length < 3;
    };
    bugBtn.onclick = () => ((feature = false), sync());
    featBtn.onclick = () => ((feature = true), sync());
    message.oninput = sync;
    send.onclick = async () => {
      if (!emailOk(email.input.value)) {
        error.textContent = t("emailBad");
        return;
      }
      error.textContent = "";
      api.setBusy(true);
      send.disabled = cancel.disabled = true;
      send.textContent = t("sending");
      try {
        await post({
          ...(await base(device.checked)),
          kind: feature ? "feature" : "bug",
          message: message.value.trim(),
          email: email.input.value.trim(),
        });
        api.setBusy(false);
        thanks(api, t("thanks"), !!email.input.value.trim());
      } catch (e) {
        api.setBusy(false);
        error.textContent = e.message;
        send.textContent = t("send");
        cancel.disabled = false;
        sync();
      }
    };
    api.box.append(
      el("h2", {}, t("title")),
      el("div", { class: "rfb-seg", role: "group" }, bugBtn, featBtn),
      prompt,
      message,
      ...email.nodes,
      el("label", { class: "rfb-check" }, device, t("device")),
      error,
      el("div", { class: "rfb-row" }, cancel, send),
      el("div", { class: "rfb-foot" }, t("footer")),
    );
    sync();
  });
}

/** Pytanie o wysłanie raportu o błędzie zapamiętanym przy poprzednim użyciu. */
export function offerPendingCrash() {
  const crash = pending();
  if (!crash || isOpen()) return;
  mount((api) => {
    const message = el("textarea", { maxlength: "8000", style: "min-height:70px" });
    const email = emailBlock();
    const error = el("div", { class: "rfb-err", role: "alert" });
    const send = el("button", { class: "rfb-primary", type: "button" }, t("crashSend"));
    const skip = el("button", { type: "button" }, t("crashSkip"));
    skip.onclick = () => {
      discard();
      api.close();
    };
    send.onclick = async () => {
      if (!emailOk(email.input.value)) {
        error.textContent = t("emailBad");
        return;
      }
      error.textContent = "";
      api.setBusy(true);
      send.disabled = skip.disabled = true;
      send.textContent = t("sending");
      try {
        const b = await base(true);
        await post({
          ...b,
          kind: "crash",
          stack: crash.stack,
          message: message.value.trim(),
          email: email.input.value.trim(),
          extra: { ...b.extra, crashedAt: crash.at, view: crash.url },
        });
        discard();
        api.setBusy(false);
        thanks(api, t("crashThanks"), !!email.input.value.trim());
      } catch (e) {
        api.setBusy(false);
        error.textContent = e.message;
        send.textContent = t("crashSend");
        send.disabled = skip.disabled = false;
      }
    };
    api.box.append(
      el("h2", {}, t("crashTitle")),
      el("p", {}, t("crashText")),
      el("details", {}, el("summary", {}, t("crashDetails")), el("pre", {}, crash.stack)),
      el("label", {}, t("crashComment")),
      message,
      ...email.nodes,
      error,
      el("div", { class: "rfb-row" }, skip, send),
    );
  });
}
