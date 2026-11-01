/* Ascend Companion — núcleo da nova interface (match-first)
 *
 * Contrato confirmado por RE do bundle original:
 *   invoke            → window.__TAURI_INTERNALS__.invoke(cmd, args)
 *   get_match_state   → { phase: "PREGAME" | "INGAME" | ... } (polling 2-4s)
 *   get_pregame / get_live_roster → { allies|myTeam, enemyTeam, mode }
 *     player: { puuid, name, tagLine, isSelf, accountLevel, competitiveTier,
 *               rank: {tier, rr, name}?, rankResolved, peakTier, peakName,
 *               peakRr?, peakResolved, leaderboardRank?, agent (uuid),
 *               partyId?, isPremade }
 *   settings_get      → objeto de settings (settings.json)
 *   settings_set      → { patch: {...} }
 *   riot_status       → { clientRunning }
 *   open_external     → { url }
 *   tracker_proxy     → { method, path, body, bearer } (transporte nativo;
 *                       base neutralizada no patch; no rebuild o transporte
 *                       vira uma webview escondida que passa no Cloudflare)
 */

const invoke = (cmd, args = {}) => window.__TAURI_INTERNALS__.invoke(cmd, args);

/* tiers Valorant (mapeamento local — sem chamada externa p/ rank) */
const TIERS = [
  "—", "—", "—", "Iron 1", "Iron 2", "Iron 3",
  "Bronze 1", "Bronze 2", "Bronze 3",
  "Silver 1", "Silver 2", "Silver 3",
  "Gold 1", "Gold 2", "Gold 3",
  "Platinum 1", "Platinum 2", "Platinum 3",
  "Diamond 1", "Diamond 2", "Diamond 3",
  "Ascendant 1", "Ascendant 2", "Ascendant 3",
  "Immortal 1", "Immortal 2", "Immortal 3",
  "Radiant",
];
const tierName = (t) => (t != null && TIERS[t] ? TIERS[t] : "—");

/* ícones vêm do CDN público preservado no patch — mesmas URLs que o
 * app original usava (media.valorant-api.com), nada de host novo */
const TIERS_UUID = "03621f52-342b-cf4e-4f86-9350a49c6d04";
const rankIcon = (t) => t == null ? "" :
  `https://media.valorant-api.com/competitivetiers/${TIERS_UUID}/${t}/smallicon.png`;
const agentIcon = (id) => id
  ? `https://media.valorant-api.com/agents/${id}/displayicon.png`
  : "";

/* vocabulário do app original (mapas/modos), rótulos pt-BR */
const MODES = {
  competitive: "Competitivo",
  unrated: "Sem Ranque",
  swiftplay: "Partida Rápida",
  spikerush: "Spike Rush",
  deathmatch: "Mata-mata",
  hurm: "Mata-mata em equipes",
  ggteam: "Escalonamento",
  onefa: "Replicação",
  newmap: "Padrão",
  fortcollins: "Retomada",
  retake: "Retomada",
  snowball: "Guerra de Bolas de Neve",
  custom: "Personalizado",
};
const MAPS = {
  ascent: "Ascent", duality: "Bind", triad: "Haven", bonsai: "Split",
  port: "Icebox", foxtrot: "Breeze", canyon: "Fracture", pitt: "Pearl",
  jam: "Lotus", juliett: "Sunset", infinity: "Abyss", rook: "Corrode",
  plummet: "Summit", range: "The Range",
};
/* o lockfile entrega caminhos UE ("/Game/Maps/Infinity/Infinity",
 * "/Game/GameModes/Deathmatch") — extrai o último segmento e normaliza */
const lastSeg = (s) => {
  const str = String(s);
  return str.includes("/")
    ? str.split("/").filter(Boolean).pop() ?? ""
    : str;
};
const modeName = (q) => {
  if (!q) return "";
  const k = lastSeg(q);
  return MODES[k.toLowerCase()] ?? k[0].toUpperCase() + k.slice(1);
};
const mapName = (k) => {
  if (!k) return "";
  const seg = lastSeg(k);
  return MAPS[seg.toLowerCase()] ?? seg;
};

/* modos sem times (FFA): uma lista só, sem aliados/inimigos */
const FFA_MODES = new Set(["deathmatch"]);

/* splash dos mapas: mapeamento autoritativo do valorant-api.com (host
 * preservado), casado pelo próprio caminho UE do lockfile; cache 7d */
const Maps = {
  byKey: null,
  async load() {
    if (Maps.byKey) return;
    try {
      const c = JSON.parse(localStorage.getItem("maps-splash") || "null");
      if (c && Date.now() - c.ts < 7 * 24 * 3600e3) {
        Maps.byKey = c.data;
        return;
      }
    } catch (e) {}
    try {
      const r = await fetch("https://valorant-api.com/v1/maps");
      const j = await r.json();
      const data = {};
      for (const m of j.data ?? []) {
        const splash = m.splash || m.displayIcon || null;
        if (!splash || !m.mapUrl) continue;
        data[m.mapUrl] = splash;
        data[lastSeg(m.mapUrl).toLowerCase()] = splash;
      }
      Maps.byKey = data;
      try {
        localStorage.setItem("maps-splash", JSON.stringify({ ts: Date.now(), data }));
      } catch (e) {}
    } catch (e) { Maps.byKey = {}; }
  },
  splash(raw) {
    if (!raw || !Maps.byKey) return "";
    return Maps.byKey[raw] ?? Maps.byKey[lastSeg(raw).toLowerCase()] ?? "";
  },
};

/* nomes de agentes (uuid → displayName): mesmo host público
 * preservado no patch, cache 7d — usado no perfil do jogador */
const Agents = {
  names: null,
  async load() {
    if (Agents.names) return;
    try {
      const c = JSON.parse(localStorage.getItem("agents-names") || "null");
      if (c && Date.now() - c.ts < 7 * 24 * 3600e3) { Agents.names = c.data; return; }
    } catch (e) {}
    try {
      const r = await fetch("https://valorant-api.com/v1/agents?isPlayableCharacter=true");
      const j = await r.json();
      const data = {};
      for (const a of j.data ?? []) data[a.uuid] = a.displayName;
      Agents.names = data;
      try { localStorage.setItem("agents-names", JSON.stringify({ ts: Date.now(), data })); } catch (e) {}
    } catch (e) { Agents.names = {}; }
  },
  name(id) {
    return id && Agents.names ? (Agents.names[id] ?? "") : "";
  },
};

/* perfil no tracker.gg — aberto no navegador (eles bloqueiam embed) */
const trackerUrl = (p) => {
  const name = p.name ?? p.gameName ?? "";
  const tag = p.tag ?? p.tagLine ?? "";
  return `https://tracker.gg/valorant/profile/riot/${encodeURIComponent(name)}%23${encodeURIComponent(tag)}/overview`;
};

/* temas: app é sempre dark, o que muda é o par de acentos —
 * --red (primário) e --mint (secundário). Como todos os tons
 * translúcidos do CSS derivam via color-mix, trocar os dois vars
 * re-tema o app inteiro (tabs, pills, cards, barras, botões). */
const THEMES = {
  valorant: { label: "Valorant", a: "#ff4655", b: "#0fd4b0" },
  oceano: { label: "Oceano", a: "#4aa8ff", b: "#2dd4bf" },
  ultra: { label: "Ultravioleta", a: "#b07cff", b: "#6ee7ff" },
  incendio: { label: "Incêndio", a: "#ff8a3d", b: "#ffd166" },
  cereja: { label: "Cereja", a: "#ff7ab8", b: "#c084fc" },
};
const HEX = /^#[0-9a-f]{6}$/i;

/* preferências de aparência/conteúdo — ficam no WebView (localStorage),
 * não vão para o backend nem para a rede. O tema também já era assim. */
const Prefs = {
  density: "comfortable",
  motion: true,
  bg: true,
  showLevel: true,
  showPeak: true,
  showLb: true,
  accent: "",
};

function readPref(key, def) {
  try {
    const raw = localStorage.getItem("crymora-" + key);
    return raw == null ? def : JSON.parse(raw);
  } catch (e) { return def; }
}
function savePref(key, value) {
  try { localStorage.setItem("crymora-" + key, JSON.stringify(value)); } catch (e) {}
}
function loadPrefs() {
  Prefs.density = readPref("density", "comfortable") === "compact" ? "compact" : "comfortable";
  Prefs.motion = readPref("motion", true) !== false;
  Prefs.bg = readPref("bg", true) !== false;
  Prefs.showLevel = readPref("showLevel", true) !== false;
  Prefs.showPeak = readPref("showPeak", true) !== false;
  Prefs.showLb = readPref("showLb", true) !== false;
  try {
    const a = localStorage.getItem("crymora-accent");
    Prefs.accent = a && HEX.test(a) ? a : "";
  } catch (e) { Prefs.accent = ""; }
}

function currentTheme() {
  try { return localStorage.getItem("crymora-theme") ?? "valorant"; }
  catch (e) { return "valorant"; }
}

/* acento personalizado: só troca --red; o --mint do tema continua como
 * cor secundária. O CSS deriva todos os tons via color-mix. */
function applyCustomAccent(hex, persist = true) {
  if (!HEX.test(String(hex))) return;
  const st = document.documentElement.style;
  st.setProperty("--red", hex);
  if (persist) {
    Prefs.accent = hex;
    try { localStorage.setItem("crymora-accent", hex); } catch (e) {}
  }
  document.querySelectorAll(".chip").forEach((c) => c.classList.remove("is-active"));
  const inp = document.querySelector("#set-accent");
  if (inp) inp.value = hex;
}

function applyTheme(key, persist = true) {
  const th = THEMES[key] ?? THEMES.valorant;
  const st = document.documentElement.style;
  st.setProperty("--red", th.a);
  st.setProperty("--mint", th.b);
  if (persist) {
    Prefs.accent = "";
    try {
      localStorage.setItem("crymora-theme", key);
      localStorage.removeItem("crymora-accent");
    } catch (e) {}
  }
  document.querySelectorAll(".chip").forEach((c) =>
    c.classList.toggle("is-active", c.dataset.theme === key));
  const inp = document.querySelector("#set-accent");
  if (inp) inp.value = th.a;
}

function applyPrefs() {
  const root = document.documentElement;
  root.dataset.density = Prefs.density;
  root.classList.toggle("motion-off", !Prefs.motion);
  root.classList.toggle("bg-flat", !Prefs.bg);
  root.classList.toggle("hide-level", !Prefs.showLevel);
  root.classList.toggle("hide-peak", !Prefs.showPeak);
  root.classList.toggle("hide-lb", !Prefs.showLb);

  if (Prefs.accent && HEX.test(Prefs.accent)) applyCustomAccent(Prefs.accent, false);
  else applyTheme(currentTheme(), false);

  const set = (id, v) => {
    const el = document.querySelector(id);
    if (el) el.checked = v;
  };
  set("#set-motion", Prefs.motion);
  set("#set-bg", Prefs.bg);
  set("#set-show-level", Prefs.showLevel);
  set("#set-show-peak", Prefs.showPeak);
  set("#set-show-lb", Prefs.showLb);
  document.querySelectorAll("#density-seg [data-density]").forEach((b) =>
    b.classList.toggle("is-active", b.dataset.density === Prefs.density));
}

loadPrefs();

/* ------------------------------------------------------------------ */
/* i18n: pt-BR e English (só esses dois)                               */
/* ------------------------------------------------------------------ */

const I18N = {
  pt: {
    tabMatch: "Partida", tabTracker: "Tracker", tabFriends: "Amigos",
    tabSettings: "Ajustes",
    tabHome: "Início",
    homeTitle: "Início", homeSub: "Sua sessão em um só lugar",
    homeConnected: "Riot Client conectado", homeOffline: "Riot Client offline",
    homeFriendsOnline: "Amigos online", homeInMatch: "Em partida",
    homeRiotClient: "Riot Client", homeOn: "on", homeOff: "off",
    homeCurrent: "Partida atual", homeNoMatch: "Nenhuma partida ativa",
    homeNoMatchSub: "Abra o Valorant ou entre na fila para começar.",
    homeOpenMatch: "Ver partida", homeOnlineNow: "Online agora",
    homeViewAll: "Ver todos", homeNoFriendsOnline: "Nenhum amigo online no momento.",
    homeSearch: "Buscar jogador", homeFriends: "Ver amigos", homeSettings: "Ajustes",
    homeSecurity: "Somente leitura · sem injeção · sem overlay",
    homeSecurityNote: "O Crymora não lê a memória do jogo, não injeta código e não automatiza nada. Seus dados ficam na sua máquina.",
    matchIdle: "Aguardando partida",
    matchIdleSub: "Abra o Valorant ou entre na fila para começar",
    matchPregame: "Pré-jogo",
    matchPregameSub: "Times montados — os dois lados visíveis",
    matchLive: "Em partida", matchLiveSub: "Ao vivo",
    pillIdle: "aguardando", pillPregame: "pré-jogo", pillLive: "ao vivo",
    friendsTitle: "Amigos",
    friendsSub: "Online primeiro — direto do cliente do Valorant",
    trackerTitle: "Tracker",
    trackerSub: "Perfil completo por Riot ID",
    trackerEmpty: "Digite um Riot ID no campo acima para ver o perfil completo.",
    trackerEmptyTitle: "Busque um jogador",
    trackerEmptyHint: "As sugestões incluem seus amigos e jogadores vistos em partidas recentes.",
    trackerSearchBtn: "Buscar",
    trackerSearchPlaceholder: "Buscar Riot ID (Nome#TAG)",
    trackerSearchResults: "Resultados",
    trackerNoResults: "Nenhum jogador encontrado localmente.",
    trackerExternalHint: "Não está na sua lista? Abra o perfil público no tracker.gg.",
    trackerOpenExternal: "Abrir {id} no tracker.gg ↗",
    trackerNeedTag: "Digite Nome#TAG para abrir um perfil externo.",
    trackerFriend: "Amigo", trackerInMatch: "Na partida", trackerSeen: "Visto recentemente",
    settingsTitle: "Ajustes", settingsSub: "Sistema, aparência e segurança",
    secSystem: "Sistema", secAppearance: "Aparência",
    secContent: "Conteúdo", secPrivacy: "Privacidade e segurança",
    setAccent: "Cor personalizada", setAccentReset: "Usar tema",
    setDensity: "Densidade", densityComfort: "Confortável", densityCompact: "Compacto",
    setMotion: "Animações", setBg: "Efeitos de fundo",
    setShowLevel: "Nível da conta", setShowPeak: "Pico de carreira", setShowLb: "Leaderboard",
    setSecurityTitle: "Somente leitura",
    setSecurityNote: "Sem memória do jogo, injeção, overlay ou automação. O app exibe os dois times no pré-jogo; veja o aviso de política da Riot no LEIA-ME.",
    setPrivacyTitle: "Conexões mínimas",
    setPrivacyNote: "Só Riot (cliente local, glz/pd) e o CDN público de ícones. Nenhuma telemetria ou backend próprio.",
    aboutEdition: "Crymora · v0.1.1",
    setBoot: "Iniciar com o Windows",
    setAlerts: "Alertas de partida", setTheme: "Cor de destaque",
    setDiscord: "Presença no Discord",
    setLang: "Idioma",
    ctxSettings: "Configurações", ctxLang: "Mudar idioma",
    ctxReload: "Recarregar interface", ctxMin: "Minimizar",
    ctxQuit: "Sair do Crymora",
    trayOpen: "Abrir Crymora", trayOn: "Riot conectado", trayOff: "Riot offline",
    ctxNa: "Não disponível nesta edição — use o botão fechar da janela",
    noMatch: "Nenhuma partida ativa — entre na fila e os times aparecem aqui.",
    noFriends: "Nenhum amigo — abra o cliente do Valorant",
    players: "Jogadores", allies: "Aliados", enemies: "Inimigos",
    you: "você",
    careerPeak: "Pico de carreira",
    lbPos: "Posição no leaderboard da região",
    watchLive: "Assistir ao vivo",
    watchLiveName: "Assistir {n} ao vivo",
    partyOf: "Grupo de {n}",
    curRank: "Atual", actPeak: "Pico do ato", unranked: "Unranked",
    actW: "V/D do ato", prevAct: "Ato anterior",
    leaderboard: "Leaderboard", careerGames: "Partidas na carreira",
    lastN: "Últimas {n}", win: "Vitória", loss: "Derrota",
    openTrackerTab: "Ver tudo na aba Tracker",
    openTrackerGg: "Abrir no tracker.gg ↗",
    viewStats: "Ver estatísticas",
    accLevel: "Nível de conta {n}",
    fIngame: "Em partida", fSelect: "Seleção de agente", fLobby: "No lobby",
    fOnline: "online", fAway: "ausente", fDnd: "ocupado", fOffline: "offline",
    selectMode: "Seleção de agente · {m}",
    toastIn: "{n} entrou em partida",
    toastSelect: "{n} está em seleção de agente",
    toastOut: "{n} saiu",
    toastNoMatch: "{n} não está mais em partida",
    close: "Fechar",
  },
  en: {
    tabMatch: "Match", tabTracker: "Tracker", tabFriends: "Friends",
    tabSettings: "Settings",
    tabHome: "Home",
    homeTitle: "Home", homeSub: "Your session at a glance",
    homeConnected: "Riot Client connected", homeOffline: "Riot Client offline",
    homeFriendsOnline: "Friends online", homeInMatch: "In match",
    homeRiotClient: "Riot Client", homeOn: "on", homeOff: "off",
    homeCurrent: "Current match", homeNoMatch: "No active match",
    homeNoMatchSub: "Open Valorant or queue up to start.",
    homeOpenMatch: "View match", homeOnlineNow: "Online now",
    homeViewAll: "View all", homeNoFriendsOnline: "No friends online right now.",
    homeSearch: "Search player", homeFriends: "View friends", homeSettings: "Settings",
    homeSecurity: "Read-only · no injection · no overlay",
    homeSecurityNote: "Crymora does not read game memory, inject code, or automate anything. Your data stays on your machine.",
    matchIdle: "Waiting for match",
    matchIdleSub: "Open Valorant or queue up to start",
    matchPregame: "Pre-game",
    matchPregameSub: "Teams are set — both sides visible",
    matchLive: "In match", matchLiveSub: "Live",
    pillIdle: "idle", pillPregame: "pregame", pillLive: "live",
    friendsTitle: "Friends",
    friendsSub: "Online first — straight from the Valorant client",
    trackerTitle: "Tracker",
    trackerSub: "Full profile by Riot ID",
    trackerEmpty: "Type a Riot ID in the field above to see the full profile.",
    trackerEmptyTitle: "Search for a player",
    trackerEmptyHint: "Suggestions include your friends and players seen in recent matches.",
    trackerSearchBtn: "Search",
    trackerSearchPlaceholder: "Search Riot ID (Name#TAG)",
    trackerSearchResults: "Results",
    trackerNoResults: "No player found locally.",
    trackerExternalHint: "Not on your list? Open the public profile on tracker.gg.",
    trackerOpenExternal: "Open {id} on tracker.gg ↗",
    trackerNeedTag: "Type Name#TAG to open an external profile.",
    trackerFriend: "Friend", trackerInMatch: "In match", trackerSeen: "Seen recently",
    settingsTitle: "Settings", settingsSub: "System, appearance & security",
    secSystem: "System", secAppearance: "Appearance",
    secContent: "Content", secPrivacy: "Privacy & security",
    setAccent: "Custom color", setAccentReset: "Use theme",
    setDensity: "Density", densityComfort: "Comfortable", densityCompact: "Compact",
    setMotion: "Animations", setBg: "Background effects",
    setShowLevel: "Account level", setShowPeak: "Career peak", setShowLb: "Leaderboard",
    setSecurityTitle: "Read-only",
    setSecurityNote: "No game memory, injection, overlay, or automation. The app shows both teams in pre-game; see the Riot policy notice in the README.",
    setPrivacyTitle: "Minimal connections",
    setPrivacyNote: "Only Riot (local client, glz/pd) and the public icon CDN. No telemetry or custom backend.",
    aboutEdition: "Crymora · v0.1.1",
    setBoot: "Launch with Windows",
    setAlerts: "Match alerts", setTheme: "Accent color",
    setDiscord: "Discord presence",
    setLang: "Language",
    ctxSettings: "Settings", ctxLang: "Switch language",
    ctxReload: "Reload interface", ctxMin: "Minimize",
    ctxQuit: "Quit Crymora",
    trayOpen: "Open Crymora", trayOn: "Riot connected", trayOff: "Riot offline",
    ctxNa: "Not available in this edition — use the window close button",
    noMatch: "No active match — queue up and the teams show up here.",
    noFriends: "No friends — open the Valorant client",
    players: "Players", allies: "Allies", enemies: "Enemies",
    you: "you",
    careerPeak: "Career peak",
    lbPos: "Regional leaderboard position",
    watchLive: "Watch live",
    watchLiveName: "Watch {n} live",
    partyOf: "Party of {n}",
    curRank: "Current", actPeak: "Act peak", unranked: "Unranked",
    actW: "Act W/L", prevAct: "Previous act",
    leaderboard: "Leaderboard", careerGames: "Career matches",
    lastN: "Last {n}", win: "Victory", loss: "Defeat",
    openTrackerTab: "See everything in the Tracker tab",
    openTrackerGg: "Open on tracker.gg ↗",
    viewStats: "View stats",
    accLevel: "Account level {n}",
    fIngame: "In match", fSelect: "Agent select", fLobby: "In lobby",
    fOnline: "online", fAway: "away", fDnd: "busy", fOffline: "offline",
    selectMode: "Agent select · {m}",
    toastIn: "{n} entered a match",
    toastSelect: "{n} is in agent select",
    toastOut: "{n} left",
    toastNoMatch: "{n} is no longer in a match",
    close: "Close",
  },
};
let LANG = "pt";
try { LANG = localStorage.getItem("crymora-lang") ?? "pt"; } catch (e) {}
const t = (key, vars) => {
  let s = (I18N[LANG] ?? I18N.pt)[key] ?? I18N.pt[key] ?? key;
  if (vars)
    for (const [k, v] of Object.entries(vars))
      s = s.replaceAll(`{${k}}`, String(v));
  return s;
};
/* textos estáticos do HTML levam data-i18n / data-i18n-title */
function applyStaticI18n() {
  document.querySelectorAll("[data-i18n]").forEach((el) => {
    el.textContent = t(el.dataset.i18n);
  });
  document.querySelectorAll("[data-i18n-title]").forEach((el) => {
    el.title = t(el.dataset.i18nTitle);
  });
  document.querySelectorAll("[data-i18n-placeholder]").forEach((el) => {
    el.placeholder = t(el.dataset.i18nPlaceholder);
  });
}
/* troca de idioma: estáticos + força re-render dos dinâmicos */
function setLang(l) {
  LANG = l === "en" ? "en" : "pt";
  try { localStorage.setItem("crymora-lang", LANG); } catch (e) {}
  document.documentElement.lang = LANG === "en" ? "en" : "pt-BR";
  document.querySelectorAll(".lang-btn[data-lang]").forEach((b) =>
    b.classList.toggle("is-active", b.dataset.lang === LANG));
  applyStaticI18n();
  State.lastPhase = null;
  State.renderSig = null;
  State.friendsSig = null;
  State.homeSig = null;
  setPhase(State.phase);
  if (State.roster) renderRoster(State.roster);
  else if ($("#view-match").classList.contains("is-active"))
    $("#teams").innerHTML = `<p class="empty">${esc(t("noMatch"))}</p>`;
  renderHome();
  tickFriends();
  renderTracker();
  if ($("#tracker-q")?.value) renderSearchResults();
}

/* ------------------------------------------------------------------ */
/* bridge                                                              */
/* ------------------------------------------------------------------ */

const Bridge = {
  async matchState() {
    try { return await invoke("get_match_state"); }
    catch (e) { return null; }
  },
  async roster(phase) {
    const cmd = phase === "PREGAME" ? "get_pregame"
              : phase === "INGAME" ? "get_live_roster" : null;
    if (!cmd) return null;
    try { return await invoke(cmd); } catch (e) { return null; }
  },
  async riotStatus() {
    try { return await invoke("riot_status"); } catch (e) { return null; }
  },
  /* roda um tick no backend agora (foco/Alt+Tab) e espera publicar */
  async refreshNow() {
    try { await invoke("refresh_now"); } catch (e) { return null; }
  },
  async settings() {
    try { return await invoke("settings_get"); } catch (e) { return {}; }
  },
  async saveSettings(patch) {
    try { await invoke("settings_set", { patch }); } catch (e) {}
  },
  openExternal(url) {
    /* sem fallback de window.open: o comando nativo valida esquema/host.
     * Se falhar, não abre nada (a permissão de opener foi removida). */
    invoke("open_external", { url }).catch((e) => diag("open_external: " + e));
  },
  /* transporte nativo: no exe original a base do tracker_proxy é
   * play-ascend.com/tracker — neutralizada p/ loopback no patch, então a
   * sonda falha instantaneamente (connection refused) e sem custo. */
  async proxyGet(path) {
    try {
      return await invoke("tracker_proxy", {
        method: "GET", path, body: null, bearer: null,
      });
    } catch (e) { return null; }
  },
};

/* ------------------------------------------------------------------ */
/* twitch: sonda tracker.gg em segundo plano                           */
/* ------------------------------------------------------------------ */

const A = "&";
const esc = (s) =>
  String(s ?? "").replace(/[&<>"']/g, (c) =>
    ({
      "&": A + "amp;",
      "<": A + "lt;",
      ">": A + "gt;",
      '"': A + "quot;",
      "'": A + "#39;",
    }[c]));

const Twitch = {
  TTL: 7 * 24 * 3600e3,   // achou canal → revalida só semanalmente
  NEG_TTL: 30 * 60e3,     // sem canal/falha → tenta de novo em 30 min
  active: 0,
  MAX: 3,                  // no máx. 3 sondas simultâneas
  cache: null,

  _load() {
    try { return JSON.parse(localStorage.getItem("twitch-cache") || "{}"); }
    catch (e) { return {}; }
  },
  persist() {
    try {
      localStorage.setItem("twitch-cache", JSON.stringify(Twitch.cache || {}));
    } catch (e) {}
  },

  riotId(p) {
    const name = p.name ?? p.gameName ?? "";
    const tag = p.tag ?? p.tagLine ?? "";
    return tag ? `${name}#${tag}` : name;
  },

  /* consulta síncrona do cache — o roster é recriado a cada tick,
   * então o botão tem que vir daqui e não de um campo no objeto */
  cached(puuid) {
    if (!Twitch.cache) Twitch.cache = Twitch._load();
    const hit = Twitch.cache[puuid];
    if (hit && hit.twitch && Date.now() - hit.ts < Twitch.TTL) {
      return hit.twitch;
    }
    return null;
  },

  profilePath(riotId) {
    const [name, tag] = riotId.split("#");
    return `/valorant/profile/riot/${encodeURIComponent(name)}%23${encodeURIComponent(tag)}/overview`;
  },

  /* Extrai o canal de Twitch do conteúdo do perfil. O tracker.gg
   * embute o estado da página (JSON) e/ou os links sociais no HTML. */
  parseChannel(content) {
    if (!content) return null;
    let text = content;
    if (typeof content === "object") {
      // proxy pode devolver JSON já-parseado (texto embutido)
      text = JSON.stringify(content);
    }
    const nextData = text.match(
      /<script id="__NEXT_DATA__"[^>]*>([\s\S]*?)<\/script>/
    );
    const source = nextData ? nextData[1] : text;
    const m = source.match(/twitch\.tv\/([A-Za-z0-9_]{3,25})/);
    if (m && !["www", "directory", "videos", "p", "downloads"].includes(m[1].toLowerCase()))
      return m[1];
    return null;
  },

  probe(puuid, riotId) {
    if (!Twitch.cache) Twitch.cache = Twitch._load();
    const hit = Twitch.cache[puuid];
    const ttl = hit && hit.twitch ? Twitch.TTL : Twitch.NEG_TTL;
    if (hit && Date.now() - hit.ts < ttl) return Promise.resolve(hit.twitch);
    return new Promise((resolve) => {
      const run = async () => {
        if (Twitch.active >= Twitch.MAX) return setTimeout(run, 350);
        Twitch.active++;
        let twitch = null;
        try {
          const res = await Bridge.proxyGet(Twitch.profilePath(riotId));
          twitch = Twitch.parseChannel(res);
        } catch (e) { /* offline/sem perfil → sem botão */ }
        Twitch.cache[puuid] = { twitch, ts: Date.now() };
        Twitch.active--;
        Twitch.persist();
        resolve(twitch);
      };
      run();
    });
  },
};

/* ------------------------------------------------------------------ */
/* state + render                                                      */
/* ------------------------------------------------------------------ */

const $ = (sel) => document.querySelector(sel);

/* ------------------------------------------------------------------ */
/* jogadores já vistos — permite buscar por nick mesmo fora da partida */
/* (fica só no WebView; nenhum dado sai da máquina)                    */
/* ------------------------------------------------------------------ */

const Known = {
  key: "crymora-known-players",
  map: {},
  dirty: false,
  load() {
    try {
      const raw = JSON.parse(localStorage.getItem(this.key) || "{}");
      this.map = raw && typeof raw === "object" ? raw : {};
    } catch (e) { this.map = {}; }
  },
  add(p) {
    if (!p?.puuid) return;
    const id = Twitch.riotId(p);
    if (!id || id === "#") return;
    const stamp = Date.now();
    const entry = {
      puuid: p.puuid,
      name: p.name ?? p.gameName ?? "",
      tagLine: p.tag ?? p.tagLine ?? "",
      avatar: p.avatar ?? "",
      accountLevel: p.accountLevel ?? 0,
      competitiveTier: p.competitiveTier ?? 0,
      rankResolved: !!p.rankResolved,
      rank: p.rank ?? null,
      peakTier: p.peakTier ?? 0,
      peakResolved: !!p.peakResolved,
      actPeakTier: p.actPeakTier ?? null,
      leaderboardRank: p.leaderboardRank ?? null,
      actWins: p.actWins ?? 0,
      actGames: p.actGames ?? 0,
      prevAct: p.prevAct ?? null,
      careerGames: p.careerGames ?? 0,
      form: Array.isArray(p.form) ? p.form : [],
      _seen: stamp,
    };
    const prev = this.map[p.puuid];
    const core = (x) => JSON.stringify({ ...x, _seen: 0 });
    if (!prev || core(prev) !== core(entry)) {
      this.map[p.puuid] = entry;
      this.dirty = true;
    } else {
      prev._seen = stamp;
    }
  },
  flush() {
    if (!this.dirty) return;
    const keep = Object.values(this.map)
      .sort((a, b) => (b._seen ?? 0) - (a._seen ?? 0))
      .slice(0, 400);
    this.map = Object.fromEntries(keep.map((p) => [p.puuid, p]));
    try { localStorage.setItem(this.key, JSON.stringify(this.map)); } catch (e) {}
    this.dirty = false;
  },
};
Known.load();

/* avatar de fallback: iniciais com matiz estável por nick, em vez de "?" */
function hueOf(name) {
  let h = 0;
  for (const ch of String(name || "?")) h = (h * 31 + ch.codePointAt(0)) % 360;
  return h;
}
function initialsOf(name) {
  const base = String(name || "?").split("#")[0].trim();
  const parts = base.split(/[\s_.-]+/).filter(Boolean);
  const s = parts.length > 1
    ? (parts[0][0] ?? "") + (parts[1][0] ?? "")
    : base.slice(0, 2);
  return (s || "?").toUpperCase();
}
function avatarHTML(src, name, cls) {
  if (src) return `<img class="${cls}" src="${esc(src)}" alt="" loading="lazy" />`;
  return `<span class="${cls} av-initial" style="--av:${hueOf(name)}" aria-hidden="true">${esc(initialsOf(name))}</span>`;
}

const State = {
  phase: "IDLE", lastPhase: null,
  roster: null, rosterSeen: null, renderSig: null,
  friends: null, friendsSig: null, selfPuuid: null,
  status: null, homeSig: null,
  trackerPlayer: null, trackerSig: null, modalPuuid: null,
};
const Friends = { seen: null };

/* toasts de transição (amigos entrando/saindo de partida) */
const Toasts = {
  enabled: true,
  push(text) {
    if (!Toasts.enabled) return;
    const host = $("#toasts");
    const el = document.createElement("div");
    el.className = "toast";
    el.textContent = text;
    host.appendChild(el);
    setTimeout(() => {
      el.classList.add("out");
      setTimeout(() => el.remove(), 350);
    }, 4200);
    while (host.children.length > 4) host.firstChild.remove();
  },
};

/* notificação nativa do Windows (plugin de notificação do Tauri, se o
 * binário original o embutiu — melhor esforço: sem o plugin, o toast
 * interno já cobre) */
const Native = {
  tried: false, granted: false,
  async notify(title, body) {
    try {
      if (!Native.tried) {
        Native.tried = true;
        let ok = await invoke("plugin:notification|is_permission_granted", {});
        if (ok !== true)
          ok = (await invoke("plugin:notification|request_permission", {})) === "granted";
        Native.granted = ok === true;
      }
      if (Native.granted)
        await invoke("plugin:notification|notify", { options: { title, body } });
    } catch (e) { /* plugin ausente — segue só com o toast interno */ }
  },
};

/* toast interno + notificação nativa quando o app está em segundo
 * plano (minimizado/outra janela em foco) */
function announce(text) {
  Toasts.push(text);
  if (!document.hasFocus()) Native.notify("Crymora", text);
}

/* ------------------------------------------------------------------ */
/* menu de contexto — substitui o padrão da WebView2 (voltar/salvar
 * como/inspecionar não servem pra nada aqui); em campos de texto o
 * menu nativo segue no lugar (copiar/colar tem que funcionar)        */
/* ------------------------------------------------------------------ */
const Ctx = {
  el: null,
  close() {
    if (Ctx.el) { Ctx.el.remove(); Ctx.el = null; }
  },
  /* quit_app existe nos dois lados: no binário original (era o "Quit"
   * do TrayMenu deles — evidência do bundle) e no rebuild (lib.rs) */
  async win(cmd, quitFallback) {
    if (quitFallback) {
      try { await invoke("quit_app"); return true; }
      catch (e) { /* comando ausente/negado */ }
    }
    try {
      await invoke(`plugin:window|${cmd}`, { label: "main" });
      return true;
    } catch (e) { /* sem permissão de janela nesta edição */ }
    Toasts.push(t("ctxNa"));
    return false;
  },
  open(x, y) {
    Ctx.close();
    const m = document.createElement("div");
    m.className = "ctx-menu";
    const mk = (label, hint, fn, cls) => {
      const b = document.createElement("button");
      b.textContent = label;
      if (hint) {
        const s = document.createElement("small");
        s.textContent = hint;
        b.appendChild(s);
      }
      if (cls) b.className = cls;
      b.addEventListener("click", () => { Ctx.close(); fn(); });
      m.appendChild(b);
    };
    mk(t("ctxSettings"), null, () => switchTab("settings"));
    mk(t("ctxLang"), LANG === "pt" ? "English" : "Português (BR)",
      () => setLang(LANG === "pt" ? "en" : "pt"));
    const sep = document.createElement("div");
    sep.className = "ctx-sep";
    m.appendChild(sep);
    mk(t("ctxReload"), null, () => location.reload());
    mk(t("ctxMin"), null, () => Ctx.win("minimize"));
    mk(t("ctxQuit"), null, () => Ctx.win("destroy", true), "danger");

    document.body.appendChild(m);
    const r = m.getBoundingClientRect();
    m.style.left = Math.max(8, Math.min(x, innerWidth - r.width - 8)) + "px";
    m.style.top = Math.max(8, Math.min(y, innerHeight - r.height - 8)) + "px";
    Ctx.el = m;
  },
};

document.addEventListener("contextmenu", (e) => {
  if (e.target.closest("input, textarea, [contenteditable]")) return;
  e.preventDefault();
  Ctx.open(e.clientX, e.clientY);
});
document.addEventListener("mousedown", (e) => {
  if (Ctx.el && !e.target.closest(".ctx-menu")) Ctx.close();
});
document.addEventListener("keydown", (e) => { if (e.key === "Escape") Ctx.close(); });
window.addEventListener("blur", () => Ctx.close());

function setPhase(phase) {
  State.phase = phase;
  const pill = $("#match-pill");
  const label =
    phase === "PREGAME" ? t("pillPregame") :
    phase === "INGAME" ? t("pillLive") : t("pillIdle");
  pill.textContent = label;
  pill.className = "pill " + (phase === "INGAME" ? "pill-live"
    : phase === "PREGAME" ? "pill-pregame" : "pill-idle");
  if (phase === State.lastPhase) return; // não pisca o subtítulo a cada tick
  State.lastPhase = phase;
  $("#match-title").textContent =
    phase === "PREGAME" ? t("matchPregame") :
    phase === "INGAME" ? t("matchLive") : t("matchIdle");
  $("#match-sub").textContent =
    phase === "PREGAME" ? t("matchPregameSub") :
    phase === "INGAME" ? t("matchLiveSub") : t("matchIdleSub");
}

function playerCard(p, isAlly, i = 0, party = null) {
  const riotId = Twitch.riotId(p);
  const tier = p.rankResolved && p.rank?.tier != null
    ? p.rank.tier
    : p.competitiveTier;
  /* tier 0 com rankResolved = unranked confirmado (não é dado faltando) */
  const rank = tier > 0 ? tierName(tier)
    : p.rankResolved ? t("unranked") : "—";
  const ico = tier > 0 ? rankIcon(tier) : "";
  /* campo do agente no roster é `agent` (contrato do bundle original),
   * com fallbacks defensivos por segurança */
  const av = agentIcon(p.agent ?? p.characterId ?? p.character);
  const rrNum = p.rankResolved && tier > 0 && p.rank?.rr != null ? p.rank.rr : null;
  const rr = rrNum != null ? `${rrNum} RR` : "";
  const rrbar = rrNum != null && !p.leaderboardRank
    ? `<i class="rrbar" title="${rrNum}/100 RR"><b style="width:${Math.max(0, Math.min(100, rrNum))}%"></b></i>`
    : "";
  /* pico de carreira — já vem no roster (peakTier/peakResolved) */
  const peakT = p.peakResolved && p.peakTier > 0 ? p.peakTier : null;
  const peak = peakT != null
    ? `<span class="peak" title="${esc(t("careerPeak"))}">${rankIcon(peakT) ? `<img class="pc-rank" src="${esc(rankIcon(peakT))}" alt="" loading="lazy" />` : ""}${esc(tierName(peakT))}</span>`
    : "";
  const lb = p.leaderboardRank
    ? `<span class="lb" title="${esc(t("lbPos"))}">#${esc(p.leaderboardRank)}</span>`
    : "";
  const tw = p.puuid ? Twitch.cached(p.puuid) : null;
  const btn = tw
    ? `<button class="twitch" data-open="https://www.twitch.tv/${esc(tw)}" title="${esc(t("watchLiveName", { n: riotId }))}" aria-label="${esc(t("watchLiveName", { n: riotId }))}"><svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true"><path d="M4 3 3 6.5V20h4v2.5h2.5L12 20h4l5-5V3H4Zm14 11-3 3h-4l-2.5 2.5V17H7V5h11v9Z"/><path d="M11 8h2v5h-2zM15.5 8h2v5h-2z"/></svg></button>`
    : "";
  /* risquinhos de grupo no lado direito (mesma party = mesma cor) */
  const pm = party
    ? `<span class="pm c${party.c}" title="${esc(t("partyOf", { n: party.size }))}">${"<i></i>".repeat(Math.min(party.size, 5))}</span>`
    : "";
  /* acento do card: true=aliado, false=inimigo, null=neutro (mata-mata) */
  const accent = isAlly === true ? "ally" : isAlly === false ? "enemy" : "neutral";
  return `<article class="pcard ${accent} ${tw ? "has-twitch" : ""} ${party ? "has-pm" : ""}" data-puuid="${esc(p.puuid ?? "")}" style="--i:${i}">
    ${avatarHTML(av, riotId, "pc-av")}
    <div class="pc-body">
      <div class="pc-top">
        <span class="pc-name">${esc(riotId)}${p.isSelf ? ` <em class="you">${esc(t("you"))}</em>` : ""}</span>${btn}
      </div>
      <div class="pc-meta">
        ${ico ? `<img class="pc-rank" src="${esc(ico)}" alt="" loading="lazy" />` : ""}
        <span class="rank">${esc(rank)}</span>
        <span class="mmr">${esc(rr)}</span>${rrbar}
        ${peak}${lb}
        ${p.accountLevel ? `<span class="lvl">nv ${esc(p.accountLevel)}</span>` : ""}
      </div>
    </div>${pm}
  </article>`;
}

/* extração única do roster — usada no render e na sonda Twitch */
function rosterPlayers(roster) {
  const allies = roster.allies ?? roster.myTeam ?? [];
  const enemies = roster.enemyTeam ?? [];
  const ffa = FFA_MODES.has(String(roster.mode ?? "").toLowerCase())
    || (!enemies.length && allies.length > 5);
  return {
    allies, enemies, ffa,
    all: ffa
      ? [...allies, ...enemies].sort((a, b) =>
          Twitch.riotId(a).localeCompare(Twitch.riotId(b)))
      : [],
  };
}

/* assinatura do roster: se nada que aparece no card mudou, nem o Twitch
 * em cache, o re-render é pulado (não re-anima cards nem quebra o hover
 * a cada tick de 2.5s) */
function rosterSig(roster) {
  const { allies, enemies } = rosterPlayers(roster);
  return State.phase + "|" + [...allies, ...enemies].map((p) =>
    [
      p.puuid, p.rank?.tier, p.rank?.rr, p.peakTier, p.agent,
      p.accountLevel, p.leaderboardRank, p.partyId,
      p.puuid ? !!Twitch.cached(p.puuid) : "",
    ].join("|")
  ).join(";");
}

/* risquinhos de grupo: uma cor por partyId com 2+ membros (mesma cor
 * nos dois times — dá pra ver o grupo de uma vez). Conta é global. */
function partyMarks(roster) {
  const { allies, enemies } = rosterPlayers(roster);
  const counts = {};
  for (const p of [...allies, ...enemies]) {
    if (!p.partyId) continue;
    counts[p.partyId] = (counts[p.partyId] ?? 0) + 1;
  }
  const colors = {};
  let c = 0;
  for (const [id, n] of Object.entries(counts)) {
    if (n > 1) colors[id] = { c: c++ % 5, size: n };
  }
  return colors;
}

/* aliados em cima, inimigos embaixo; mata-mata (FFA) = lista única */
function renderRoster(roster) {
  const sig = rosterSig(roster);
  if (sig === State.renderSig) return; // nada mudou → mantém DOM (e hover)
  State.renderSig = sig;
  const { allies, enemies, ffa, all } = rosterPlayers(roster);
  const parties = partyMarks(roster);
  const section = (title, cls, cards, count = 0) => `<section class="team" aria-label="${title}">
      <h2 class="team-h ${cls}">${title} <span class="t-count">${count}</span></h2>
      ${cards || `<p class="empty">—</p>`}
    </section>`;
  const grid = (list, isAlly) =>
    list.length
      ? `<div class="cards grid">${list
          .map((p, i) => playerCard(p, isAlly, i, p.partyId ? parties[p.partyId] : null))
          .join("")}</div>`
      : "";
  if (ffa) {
    $("#teams").innerHTML = section(t("players"), "neutral", grid(all, null), all.length);
    return;
  }
  /* pré-jogo e partida: o backend devolve os dois times */
  const enemyCards = grid(enemies, false);
  $("#teams").innerHTML =
    section(t("allies"), "allies", grid(allies, true), allies.length) +
    section(t("enemies"), "enemies", enemyCards, enemies.length);
}

/* ------------------------------------------------------------------ */
/* perfil completo: modal no clique do card + aba Tracker              */
/* ------------------------------------------------------------------ */

/* amigo → formato aceito pelo profileHTML (mesmos campos do roster) */
function friendAsPlayer(f) {
  return {
    puuid: f.puuid,
    gameName: f.gameName,
    tagLine: f.tagLine,
    avatar: f.playerCardId
      ? `https://media.valorant-api.com/playercards/${f.playerCardId}/smallart.png`
      : "",
    accountLevel: f.accountLevel,
    competitiveTier: f.competitiveTier,
    rankResolved: !!f.rankResolved,
    rank: f.rankResolved
      ? { tier: f.competitiveTier, rr: f.rankedRating ?? 0 }
      : null,
    peakTier: f.peakTier,
    peakResolved: !!f.peakResolved,
    actPeakTier: f.actPeakTier,
    leaderboardRank: f.leaderboardRank,
    actWins: f.actWins ?? 0,
    actGames: f.actGames ?? 0,
    prevAct: f.prevAct ?? {},
    careerGames: f.careerGames ?? 0,
    form: Array.isArray(f.form) ? f.form : [],
  };
}

function findPlayer(puuid) {
  if (!puuid) return null;
  if (State.roster) {
    const { allies, enemies } = rosterPlayers(State.roster);
    const hit = [...allies, ...enemies].find((p) => p.puuid === puuid);
    if (hit) return hit;
  }
  const friend = State.friends?.find((f) => f.puuid === puuid);
  if (friend) return friendAsPlayer(friend);
  return Known.map[puuid] ?? null;
}

const pct = (w, g) => g > 0 ? Math.round((w / g) * 100) + "%" : "—";

/* form[] do roster: resultados recentes — true/1/"W" = vitória */
const isWin = (x) => x === true || x === 1 ||
  String(x ?? "").toUpperCase().startsWith("W");

/* mesmo HTML serve pro modal e pra aba Tracker (inModal adiciona
 * o botão que leva à aba e o × de fechar) */
function profileHTML(p, inModal) {
  const riotId = Twitch.riotId(p);
  const tier = p.rankResolved && p.rank?.tier != null ? p.rank.tier : p.competitiveTier;
  const av = p.avatar || agentIcon(p.agent ?? p.characterId ?? p.character);
  const tw = p.puuid ? Twitch.cached(p.puuid) : null;
  const actTier = p.actPeakTier ?? null;
  const prev = p.prevAct ?? {};
  const form = Array.isArray(p.form) ? p.form.slice(-5).reverse() : [];
  /* linha de identidade: nível de conta (só quando realmente conhecido)
   * + agente da partida. Nível 0 = presença offline sem cache → omite. */
  const bits = [];
  if (Prefs.showLevel && p.accountLevel != null && p.accountLevel > 0)
    bits.push(`<span class="lvl">${t("accLevel", { n: esc(p.accountLevel) })}</span>`);
  const agentNm = Agents.name(p.agent ?? p.characterId ?? p.character);
  if (agentNm) bits.push(esc(agentNm));
  const rankCell = (label, tierVal, sub, cls = "", resolved = false) => {
    const tv = tierVal != null && tierVal > 0 ? tierVal : null;
    if (tv == null) {
      const v = resolved ? t("unranked") : "—";
      return `<div class="tr-cell ${cls}"><span class="tr-k">${label}</span><span class="tr-v ${resolved ? "" : "dim"}">${esc(v)}</span></div>`;
    }
    return `<div class="tr-cell ${cls}">
        <img class="pc-rank" src="${esc(rankIcon(tv))}" alt="" loading="lazy" />
        <div><span class="tr-k">${label}</span><span class="tr-v">${esc(tierName(tv))}${sub ? ` <em>${esc(sub)}</em>` : ""}</span></div>
      </div>`;
  };
  const statCell = (k, v, cls = "") => `<div class="tr-cell ${cls}"><span class="tr-k">${k}</span><span class="tr-v">${esc(v)}</span></div>`;
  return `<div class="tr-head">
      ${avatarHTML(av, riotId, "tr-av")}
      <div class="tr-id">
        <h2>${esc(riotId)}${p.isSelf ? ` <em class="you">${esc(t("you"))}</em>` : ""}</h2>
        ${bits.length ? `<p class="tr-sub">${bits.join(" · ")}</p>` : ""}
      </div>
      ${tw ? `<button class="twitch" data-open="https://www.twitch.tv/${esc(tw)}" title="${esc(t("watchLive"))}"><svg viewBox="0 0 24 24" width="14" height="14" fill="currentColor" aria-hidden="true"><path d="M4 3 3 6.5V20h4v2.5h2.5L12 20h4l5-5V3H4Zm14 11-3 3h-4l-2.5 2.5V17H7V5h11v9Z"/><path d="M11 8h2v5h-2zM15.5 8h2v5h-2z"/></svg></button>` : ""}
      ${inModal ? `<button class="tr-close" data-close="1" title="${esc(t("close"))}" aria-label="${esc(t("close"))}">✕</button>` : ""}
    </div>
    <div class="tr-ranks">
      ${rankCell(t("curRank"), tier, p.rankResolved && tier > 0 && p.rank?.rr != null ? `${p.rank.rr} RR` : "", "cur-cell", p.rankResolved)}
      ${rankCell(t("careerPeak"), p.peakResolved ? p.peakTier : null, "", "peak-cell", p.peakResolved)}
      ${rankCell(t("actPeak"), actTier, "", "actpeak-cell", actTier != null)}
    </div>
    <div class="tr-stats">
      ${statCell(t("actW"), `${p.actWins ?? 0}/${p.actGames ?? 0} (${pct(p.actWins ?? 0, p.actGames ?? 0)})`)}
      ${statCell(t("prevAct"), prev.games ? `${prev.tier != null ? tierName(prev.tier) + " · " : ""}${prev.wins ?? 0}/${prev.games}` : "—")}
      ${statCell(t("leaderboard"), p.leaderboardRank ? `#${p.leaderboardRank}` : "—", "lb-cell")}
      ${statCell(t("careerGames"), p.careerGames ?? "—")}
    </div>
    ${form.length ? `<div class="tr-form">
      <span class="tr-k">${esc(t("lastN", { n: form.length }))}</span>
      ${form.map((x) => `<i class="${isWin(x) ? "w" : "l"}" title="${esc(isWin(x) ? t("win") : t("loss"))}"></i>`).join("")}
    </div>` : ""}
    <div class="tr-actions">
      ${inModal ? `<button class="btn" data-tracker="1">${esc(t("openTrackerTab"))}</button>` : ""}
      <button class="btn primary" data-open="${esc(trackerUrl(p))}">${esc(t("openTrackerGg"))}</button>
    </div>`;
}

function openDetails(puuid) {
  const p = findPlayer(puuid);
  if (!p) return;
  State.modalPuuid = puuid;
  $("#modal-card").innerHTML = profileHTML(p, true);
  const m = $("#modal");
  m.hidden = false;
  requestAnimationFrame(() => m.classList.add("open"));
}

function closeModal() {
  const m = $("#modal");
  m.classList.remove("open");
  setTimeout(() => { m.hidden = true; }, 200);
}

/* toggles de conteúdo (nível/pico/leaderboard) re-renderizam os perfis
 * já abertos — o CSS esconde as células, mas o HTML é gerado com os bits
 * de identidade concatenados, então o modal/Tracker precisa ser refeito. */
function refreshProfileViews() {
  renderTracker();
  if (!$("#modal").hidden && State.modalPuuid) {
    const p = findPlayer(State.modalPuuid);
    if (p) $("#modal-card").innerHTML = profileHTML(p, true);
  }
}

function renderTracker() {
  const body = $("#tracker-body");
  if (!body) return;
  if (!State.trackerPlayer) {
    body.innerHTML = `<div class="tr-empty">
      <span class="tr-empty-ico" aria-hidden="true">
        <svg viewBox="0 0 24 24" width="26" height="26" fill="none"><circle cx="11" cy="11" r="6.5" stroke="currentColor" stroke-width="1.7"/><path d="m16 16 4 4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round"/></svg>
      </span>
      <p class="tr-empty-title">${esc(t("trackerEmptyTitle"))}</p>
      <p>${esc(t("trackerEmpty"))}</p>
      <p class="tr-empty-hint">${esc(t("trackerEmptyHint"))}</p>
    </div>`;
    return;
  }
  body.innerHTML = profileHTML(State.trackerPlayer, false);
}

/* ------------------------------------------------------------------ */
/* busca do Tracker — local primeiro (amigos, partida e já vistos)     */
/* ------------------------------------------------------------------ */

function parseRiotId(q) {
  const s = String(q ?? "").trim();
  const i = s.lastIndexOf("#");
  if (i > 0) return { name: s.slice(0, i).trim(), tag: s.slice(i + 1).trim() };
  return { name: s, tag: "" };
}

function searchPlayers(query) {
  const q = String(query ?? "").trim().toLowerCase();
  if (!q) return [];
  const pool = new Map();
  const add = (p, src) => {
    if (!p?.puuid || pool.has(p.puuid)) return;
    const id = Twitch.riotId(p);
    if (!id || id === "#") return;
    pool.set(p.puuid, { p, src });
  };
  if (State.roster) {
    const { allies, enemies } = rosterPlayers(State.roster);
    for (const p of [...allies, ...enemies]) add(p, "match");
  }
  for (const f of State.friends ?? []) add(friendAsPlayer(f), "friend");
  for (const p of Object.values(Known.map)) add(p, "seen");

  const idOf = (p) => Twitch.riotId(p).toLowerCase();
  const nameOf = (p) => String(p.name ?? p.gameName ?? "").toLowerCase();
  const tagOf = (p) => String(p.tag ?? p.tagLine ?? "").toLowerCase();
  const exact = (p) => idOf(p) === q ? 1 : 0;
  const starts = (p) => (idOf(p).startsWith(q) || nameOf(p).startsWith(q)) ? 1 : 0;
  const srcRank = { match: 0, friend: 1, seen: 2 };
  return [...pool.values()]
    .filter(({ p }) => idOf(p).includes(q) || nameOf(p).includes(q) || tagOf(p).includes(q))
    .sort((a, b) =>
      exact(b.p) - exact(a.p) ||
      starts(b.p) - starts(a.p) ||
      (srcRank[a.src] ?? 9) - (srcRank[b.src] ?? 9) ||
      Twitch.riotId(a.p).localeCompare(Twitch.riotId(b.p))
    )
    .slice(0, 8);
}

function hideSearchResults() {
  const box = $("#tracker-results");
  if (!box) return;
  box.hidden = true;
  box.innerHTML = "";
}

function selectTracker(p) {
  if (!p) return;
  State.trackerPlayer = p;
  State.trackerSig = null;
  const q = $("#tracker-q");
  if (q) q.value = Twitch.riotId(p);
  hideSearchResults();
  renderTracker();
}

function renderSearchResults() {
  const qEl = $("#tracker-q");
  const box = $("#tracker-results");
  if (!qEl || !box) return;
  const raw = qEl.value.trim();
  if (!raw) {
    hideSearchResults();
    renderTracker();
    return;
  }
  const results = searchPlayers(raw);
  const parsed = parseRiotId(raw);
  const srcLabel = { match: t("trackerInMatch"), friend: t("trackerFriend"), seen: t("trackerSeen") };
  const rows = results.map(({ p, src }) => {
    const id = Twitch.riotId(p);
    const tier = p.rankResolved && p.rank?.tier != null ? p.rank.tier : p.competitiveTier;
    const meta = [tier > 0 ? tierName(tier) : "", srcLabel[src] ?? ""].filter(Boolean).join(" · ");
    const av = p.avatar || agentIcon(p.agent ?? p.characterId ?? p.character);
    return `<button class="tresult" type="button" data-tracker-puuid="${esc(p.puuid)}">
      ${avatarHTML(av, id, "t-av")}
      <span class="t-id"><b>${esc(id)}</b><small>${esc(meta)}</small></span>
      <span class="t-go" aria-hidden="true">↵</span>
    </button>`;
  }).join("");
  const external = parsed.tag
    ? `<button class="tresult external" type="button" data-open="${esc(trackerUrl({ name: parsed.name, tag: parsed.tag }))}">
        <span class="t-ext" aria-hidden="true">↗</span>
        <span class="t-id"><b>${esc(t("trackerOpenExternal", { id: `${parsed.name}#${parsed.tag}` }))}</b><small>${esc(t("trackerExternalHint"))}</small></span>
      </button>`
    : `<div class="tresult-empty">${esc(t("trackerNeedTag"))}</div>`;
  box.innerHTML =
    `<div class="tresult-head">${esc(t("trackerSearchResults"))} · ${results.length}</div>` +
    (rows || `<div class="tresult-empty">${esc(t("trackerNoResults"))}</div>`) +
    external;
  box.hidden = false;
}

/* troca de aba isolada — usada pelo clique nos tabs e pelo botão do modal */
function switchTab(view) {
  const isHome = view === "home";
  document.querySelectorAll(".tab").forEach((x) =>
    x.classList.toggle("is-active", !isHome && x.dataset.view === view));
  $("#home-btn")?.classList.toggle("is-active", isHome);
  document.querySelectorAll(".view").forEach((v) =>
    v.classList.toggle("is-active", v.id === "view-" + view));
  if (view === "home") renderHome();
  if (view === "friends") tickFriends();
  if (view === "tracker") {
    renderTracker();
    if (!State.trackerPlayer) setTimeout(() => $("#tracker-q")?.focus(), 60);
  }
}

/* ------------------------------------------------------------------ */
/* início: visão geral da sessão (aberta pelo logo do rail)            */
/* ------------------------------------------------------------------ */

function renderHome() {
  const body = $("#home-body");
  if (!body) return;
  const st = State.status ?? {};
  const friends = State.friends ?? [];
  const groupOrder = { ingame: 0, select: 1, lobby: 2, online: 3, away: 4 };
  const online = friends
    .filter((f) => groupOrder[friendInfo(f).group] != null)
    .sort((a, b) =>
      (groupOrder[friendInfo(a).group] ?? 9) - (groupOrder[friendInfo(b).group] ?? 9) ||
      Twitch.riotId(a).localeCompare(Twitch.riotId(b)));
  const inMatch = friends.filter((f) => {
    const g = friendInfo(f).group;
    return g === "ingame" || g === "select";
  }).length;
  const phase = State.phase;
  const connected = !!st.clientRunning;
  const selfName = st.selfName || "—";
  const region = st.region
    ? String(st.region).toUpperCase() + (st.shard ? " · " + st.shard : "")
    : "—";
  const phaseLabel = phase === "PREGAME" ? t("matchPregame")
    : phase === "INGAME" ? t("matchLive") : t("matchIdle");
  const roster = State.roster;
  const rp = roster ? rosterPlayers(roster) : null;
  const matchLine = rp
    ? [modeName(roster.mode), mapName(roster.mapKey ?? roster.map)].filter(Boolean).join(" · ")
    : "";
  /* FFA (mata-mata) não tem aliados/inimigos — o card mostra o total */
  const countText = rp?.ffa
    ? `${rp.all.length} ${t("players").toLowerCase()}`
    : `${rp ? rp.allies.length : 0} ${t("allies").toLowerCase()} · ${
        rp ? rp.enemies.length : 0
      } ${t("enemies").toLowerCase()}`;

  const onlineRows = online.slice(0, 5).map((f) => {
    const info = friendInfo(f);
    const p = friendAsPlayer(f);
    return `<button class="home-friend" type="button" data-puuid="${esc(f.puuid)}">
      ${avatarHTML(p.avatar, Twitch.riotId(f), "f-av")}
      <span class="home-friend-id"><b>${esc(Twitch.riotId(f))}</b><small>${esc(info.label)}</small></span>
      <span class="f-dot ${info.cls}" aria-hidden="true"></span>
    </button>`;
  }).join("");

  const sig = JSON.stringify([
    phase, connected, selfName, region, st.shard,
    online.map((f) => f.puuid).join(","), inMatch,
    roster ? [roster.mode, roster.mapKey, rp?.allies.length, rp?.enemies.length, rp?.ffa] : null,
    LANG,
  ]);
  if (sig === State.homeSig) return;
  State.homeSig = sig;

  const pill = $("#home-pill");
  if (pill) {
    pill.textContent = phase === "PREGAME" ? t("pillPregame")
      : phase === "INGAME" ? t("pillLive") : t("pillIdle");
    pill.className = "pill " + (phase === "INGAME" ? "pill-live"
      : phase === "PREGAME" ? "pill-pregame" : "pill-idle");
  }

  body.innerHTML = `
    <section class="home-hero">
      <img class="home-logo" src="/assets/logo.png" alt="" />
      <div class="home-id">
        <span class="home-kicker ${connected ? "on" : ""}">${esc(connected ? t("homeConnected") : t("homeOffline"))}</span>
        <h2>${esc(selfName)}</h2>
        <p class="sub">${esc(region)} · ${esc(phaseLabel)}</p>
      </div>
      <span class="home-dot ${connected ? "on" : ""}" aria-hidden="true"></span>
    </section>
    <div class="home-stats">
      <div class="home-stat"><span class="home-stat-v">${online.length}</span><span class="home-stat-k">${esc(t("homeFriendsOnline"))}</span></div>
      <div class="home-stat"><span class="home-stat-v">${inMatch}</span><span class="home-stat-k">${esc(t("homeInMatch"))}</span></div>
      <div class="home-stat"><span class="home-stat-v">${esc(connected ? t("homeOn") : t("homeOff"))}</span><span class="home-stat-k">${esc(t("homeRiotClient"))}</span></div>
    </div>
    <section class="home-card">
      <div class="home-card-head"><h3>${esc(t("homeCurrent"))}</h3></div>
      ${rp
        ? `<p class="home-match">${esc(matchLine || "—")}</p>
           <p class="sub">${esc(countText)}</p>
           <button class="btn primary" type="button" data-goto="match">${esc(t("homeOpenMatch"))}</button>`
        : `<p class="home-match">${esc(t("homeNoMatch"))}</p>
           <p class="sub">${esc(t("homeNoMatchSub"))}</p>`}
    </section>
    <section class="home-card">
      <div class="home-card-head">
        <h3>${esc(t("homeOnlineNow"))}</h3>
        <button class="link-btn" type="button" data-goto="friends">${esc(t("homeViewAll"))}</button>
      </div>
      ${onlineRows || `<p class="empty">${esc(t("homeNoFriendsOnline"))}</p>`}
    </section>
    <div class="home-actions">
      <button class="btn" type="button" data-goto="tracker" data-focus-search="1">${esc(t("homeSearch"))}</button>
      <button class="btn" type="button" data-goto="friends">${esc(t("homeFriends"))}</button>
      <button class="btn" type="button" data-goto="settings">${esc(t("homeSettings"))}</button>
    </div>
    <div class="home-sec">
      <span class="shield" aria-hidden="true"><svg viewBox="0 0 24 24" width="15" height="15" fill="none"><path d="M12 3 5 6v5c0 4.4 2.8 8.2 7 9.5 4.2-1.3 7-5.1 7-9.5V6l-7-3Z" stroke="currentColor" stroke-width="1.7" stroke-linejoin="round"/><path d="m9 12 2 2 4-4" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg></span>
      <span>${esc(t("homeSecurityNote"))}</span>
    </div>`;
}

/* ------------------------------------------------------------------ */
/* amigos: backend local (lockfile do cliente Riot) — sem rede        */
/* ------------------------------------------------------------------ */

/* árvore do app original, ampliada: em partida → seleção de agente →
 * lobby → online → ausente → offline; quem está jogando mostra
 * modo · mapa · placar (contrato: queueId, mapKey, scoreAlly/Enemy) */
function friendInfo(f) {
  if (f.sessionState === "INGAME") {
    const parts = [modeName(f.queueId)];
    const m = mapName(f.mapKey);
    if (m) parts.push(m);
    if (f.scoreAlly != null && f.scoreEnemy != null)
      parts.push(`${f.scoreAlly}–${f.scoreEnemy}`);
    return { group: "ingame", label: parts.filter(Boolean).join(" · "), cls: "live" };
  }
  if (f.sessionState === "PREGAME") {
    const m = modeName(f.queueId);
    return {
      group: "select",
      label: m ? t("selectMode", { m }) : t("fSelect"),
      cls: "live",
    };
  }
  if (f.product === "valorant") return { group: "lobby", label: t("fLobby"), cls: "on" };
  // "chat" e "online" = conectado; "mobile" = pelo celular
  if (f.availability === "chat" || f.availability === "online" || f.availability === "mobile")
    return { group: "online", label: t("fOnline"), cls: "on" };
  if (f.availability === "away") return { group: "away", label: t("fAway"), cls: "away" };
  if (f.availability === "dnd") return { group: "away", label: t("fDnd"), cls: "away" };
  return { group: "offline", label: t("fOffline"), cls: "off" };
}

/* [chave do grupo, chave i18n do título] — grupo interno nunca muda */
const F_GROUPS = [
  ["ingame", "fIngame"],
  ["select", "fSelect"],
  ["lobby", "fLobby"],
  ["online", "fOnline"],
  ["away", "fAway"],
  ["offline", "fOffline"],
];

function friendRow(f, label, cls, i = 0) {
  const id = `${f.gameName ?? f.name ?? ""}${f.tagLine ? "#" + f.tagLine : ""}`;
  const party = (f.partySize ?? 0) > 1
    ? `<span class="f-party" title="${esc(t("partyOf", { n: f.partySize }))}">×${esc(f.partySize)}</span>`
    : "";
  const tier = (f.competitiveTier ?? 0) > 0 ? f.competitiveTier : null;
  const ico = tier ? rankIcon(tier) : "";
  const rankTitle = tier
    ? `${tierName(tier)}${f.rankedRating != null ? ` · ${f.rankedRating} RR` : ""}`
    : "";
  const card = f.playerCardId
    ? `https://media.valorant-api.com/playercards/${f.playerCardId}/smallart.png`
    : "";
  return `<article class="frow ${cls}" style="--i:${i}" data-puuid="${esc(f.puuid ?? "")}"
      role="button" tabindex="0" title="${esc(t("viewStats"))}">
    <span class="f-dot" aria-hidden="true"></span>
    ${avatarHTML(card, id, "f-av")}
    <span class="f-name">${esc(id)}</span>${party}
    ${ico ? `<img class="f-rank" src="${esc(ico)}" alt="" loading="lazy" title="${esc(rankTitle)}" />` : ""}
    <span class="f-status">${esc(label)}</span>
  </article>`;
}

async function tickFriends() {
  if (document.hidden) return;
  let res;
  try { res = await invoke("get_friends"); } catch (e) { diag("friends: invoke falhou: " + e); return; }
  const list = res?.friends ?? [];
  State.friends = list;
  if (!res?.friends) diag("friends: payload " + JSON.stringify(res));
  for (const f of list) Known.add(friendAsPlayer(f));
  Known.flush();

  /* toasts de transição: só as mudanças de estado importam */
  const nowStates = {};
  for (const f of list) nowStates[f.puuid] = friendInfo(f).group;
  if (Friends.seen) {
    const prevStates = Friends.seen;
    for (const f of list) {
      const g = nowStates[f.puuid], was = prevStates[f.puuid];
      if (was === g) continue;
      const who = Twitch.riotId(f);
      if (!was) continue; // jogador que apareceu na lista agora — sem toast
      if (g === "ingame" && was !== "ingame")
        announce(t("toastIn", { n: who }));
      else if (g === "select" && was !== "select")
        announce(t("toastSelect", { n: who }));
      else if (was === "ingame" || was === "select")
        announce(g === "offline" ? t("toastOut", { n: who })
                                 : t("toastNoMatch", { n: who }));
    }
  }
  Friends.seen = nowStates;

  /* assinatura: se nada mudou (grupo, elo, label, card), pula o re-render —
   * não re-anima as linhas a cada tick de 5s */
  const sig = list.map((f) => {
    const i = friendInfo(f);
    return `${f.puuid}|${i.group}|${i.label}|${f.competitiveTier}|${f.rankedRating}|${f.partySize}|${f.accountLevel}|${f.playerCardId}`;
  }).sort().join(";");
  const active = $("#view-friends").classList.contains("is-active");
  if (!active) {
    State.friendsSig = sig;
    renderHome();
    return;
  }
  if (sig === State.friendsSig && $("#friends-list").children.length) {
    renderHome();
    return;
  }
  State.friendsSig = sig;

  const byGroup = {};
  for (const f of list) {
    const info = friendInfo(f);
    (byGroup[info.group] ??= []).push({ f, ...info });
  }
  for (const arr of Object.values(byGroup))
    arr.sort((a, b) => Twitch.riotId(a.f).localeCompare(Twitch.riotId(b.f)));
  $("#friends-list").innerHTML =
    F_GROUPS.map(([key, titleKey]) => {
      const arr = byGroup[key] ?? [];
      if (!arr.length) return "";
      const rows = arr.map((x, i) => friendRow(x.f, x.label, x.cls, i)).join("");
      return `<h3 class="f-head">${esc(t(titleKey))} <span class="f-count">${arr.length}</span></h3>` + rows;
    }).join("") ||
    `<p class="empty">${esc(t("noFriends"))}</p>`;
  renderHome();
}

/* sonda Twitch só para jogadores novos desta partida */
async function probeNewPlayers(roster) {
  const { allies, enemies } = rosterPlayers(roster);
  const players = [...allies, ...enemies];
  const sig = players.map((p) => p.puuid).sort().join(",");
  if (sig === State.rosterSeen) return;
  State.rosterSeen = sig;
  await Promise.all(
    players.filter((p) => p.puuid && Twitch.riotId(p)).map((p) =>
      Twitch.probe(p.puuid, Twitch.riotId(p))
    )
  );
  renderRoster(roster); // re-render: botões saem do cache agora
}

/* ------------------------------------------------------------------ */
/* polling leve (mesmo desenho do app original)                        */
/* ------------------------------------------------------------------ */

/* splash do mapa no cabeçalho da partida (fade escuro por cima p/
 * legibilidade do texto) */
function applySplash(raw) {
  const bar = $("#match-bar");
  const url = Maps.splash(raw);
  bar.style.backgroundImage = url
    ? `linear-gradient(rgba(11,13,16,.86), rgba(11,13,16,.55)), url("${url}")`
    : "";
  bar.classList.toggle("has-splash", !!url);
}

let lastTickDiag = 0;
async function tick() {
  /* em segundo plano o WebView estrangula timers; ao voltar, o handler
   * de foco roda refresh_now + tick na hora */
  if (document.hidden) return;
  const [st, rs] = await Promise.all([Bridge.matchState(), Bridge.riotStatus()]);
  const now = Date.now();
  if (!st || now - lastTickDiag > 15000) {
    lastTickDiag = now;
    diag("tick: matchState=" + (st ? JSON.stringify(st) : "null"));
  }
  if (!st) return;
  const phase = st.phase ?? null;
  setPhase(phase ?? "IDLE");
  State.status = rs ?? null;
  $("#conn").classList.toggle("on", !!rs?.clientRunning);

  /* troca de conta no Riot Client: descarta perfil/amigos da sessão antiga.
   * O backend relê o token a cada tick e zera a região; aqui só limpamos
   * o que ficou renderizado na tela. */
  const selfId = rs?.puuid ?? "";
  if (State.selfPuuid !== null && selfId !== State.selfPuuid) {
    State.trackerPlayer = null;
    State.trackerSig = null;
    State.modalPuuid = null;
    State.friends = null;
    State.friendsSig = null;
    State.rosterSeen = null;
    State.renderSig = null;
    State.homeSig = null;
    Friends.seen = null;
    /* nada de jogadores da conta anterior na busca local */
    Known.map = {};
    try { localStorage.removeItem(Known.key); } catch (e) {}
    const q = $("#tracker-q");
    if (q) q.value = "";
    hideSearchResults();
    $("#friends-list").innerHTML = "";
    closeModal();
    renderTracker();
  }
  State.selfPuuid = selfId;

  if (phase === "PREGAME" || phase === "INGAME") {
    const roster = await Bridge.roster(phase);
    if (roster) {
      State.roster = roster;
      const rp = rosterPlayers(roster);
      for (const p of [...rp.allies, ...rp.enemies]) Known.add(p);
      Known.flush();
      /* aba Tracker aberta? mantém o perfil do jogador selecionado fresco
       * (mas só re-renderiza se algo que aparece lá mudou) */
      if (State.trackerPlayer?.puuid) {
        const fresh = [...rp.allies, ...rp.enemies]
          .find((p) => p.puuid === State.trackerPlayer.puuid);
        const sig = fresh
          ? JSON.stringify([
              fresh.rank?.tier, fresh.rank?.rr, fresh.rankResolved,
              fresh.peakTier, fresh.actPeakTier, fresh.actWins,
              fresh.actGames, fresh.accountLevel, fresh.leaderboardRank,
              fresh.form, fresh.prevAct, fresh.careerGames,
              fresh.puuid ? +!!Twitch.cached(fresh.puuid) : 0,
            ])
          : null;
        if (sig != null && sig !== State.trackerSig &&
            $("#view-tracker").classList.contains("is-active")) {
          State.trackerSig = sig;
          State.trackerPlayer = fresh;
          renderTracker();
        }
      }
      renderRoster(roster);
      applySplash(roster.mapKey ?? roster.map);
      const sub = [
        modeName(roster.mode),
        mapName(roster.mapKey ?? roster.map),
      ].filter(Boolean).join(" · ");
      if (sub) $("#match-sub").textContent = sub;
      probeNewPlayers(roster);
    }
  } else {
    State.roster = null;
    State.rosterSeen = null;
    State.renderSig = null; // próxima partida re-anima os cards
    $("#teams").innerHTML = `<p class="empty">${esc(t("noMatch"))}</p>`;
    applySplash(null);
  }
  renderHome();
}

/* Alt+Tab: além de o WebView estrangular os timers em segundo plano, o
 * backend pode estar no meio do intervalo. Aqui rodamos um tick na hora
 * e só então lemos o snapshot — a tela volta já atualizada. */
let focusBusy = false;
let lastFocusAt = 0;
async function refreshOnFocus() {
  if (document.hidden) return;
  const now = Date.now();
  if (focusBusy || now - lastFocusAt < 300) return;
  lastFocusAt = now;
  focusBusy = true;
  try {
    await Bridge.refreshNow();
    await Promise.all([tick(), tickFriends()]);
  } finally {
    focusBusy = false;
  }
}

/* ------------------------------------------------------------------ */
/* boot                                                                */
/* ------------------------------------------------------------------ */

async function boot() {
  const s = await Bridge.settings();
  $("#set-boot").checked = !!s.launchOnBoot;
  $("#set-alerts").checked = !(s.trackedInGameAlertsEnabled === false);
  $("#set-discord").checked = !!s.discordRpEnabled;
  Toasts.enabled = !(s.trackedInGameAlertsEnabled === false);
  $("#app-version").textContent = t("aboutEdition");

  /* renomear a janela pro nome novo (o exe em si continua
   * ascend-companion.exe até o rebuild — atalho/autostart dependem dele) */
  document.title = "Crymora";
  invoke("plugin:window|set_title", { label: "main", title: "Crymora" })
    .catch(() => { /* sem permissão p/ isso — título nativo segue o original */ });

  Maps.load(); // splash dos mapas (cache 7d, host preservado)
  Agents.load(); // nomes de agentes p/ o perfil

  /* temas — chips bicolor, o marcado leva o acento até o card */
  const chips = $("#theme-chips");
  chips.innerHTML = Object.entries(THEMES).map(([k, t]) =>
    `<button class="chip" data-theme="${k}" title="${esc(t.label)}" aria-label="Tema ${esc(t.label)}"><i style="background:${t.a}"></i><i style="background:${t.b}"></i></button>`
  ).join("");
  applyPrefs();
  chips.addEventListener("click", (e) => {
    const c = e.target.closest(".chip");
    if (c) applyTheme(c.dataset.theme);
  });

  /* idioma — pt/en; estáticos já vêm em pt no HTML, o i18n sobrescreve */
  document.documentElement.lang = LANG === "en" ? "en" : "pt-BR";
  applyStaticI18n();
  document.querySelectorAll(".lang-btn[data-lang]").forEach((b) =>
    b.classList.toggle("is-active", b.dataset.lang === LANG));
  $("#lang-seg")?.addEventListener("click", (e) => {
    const b = e.target.closest(".lang-btn[data-lang]");
    if (b) setLang(b.dataset.lang);
  });

  $("#tabs").addEventListener("click", (e) => {
    const tab = e.target.closest(".tab");
    if (tab) switchTab(tab.dataset.view);
  });
  $("#home-btn").addEventListener("click", () => switchTab("home"));

  /* busca do Tracker: local-first, com fallback externo para Nome#TAG */
  const trackerInput = $("#tracker-q");
  const runSearch = () => {
    if (!trackerInput?.value.trim()) {
      hideSearchResults();
      renderTracker();
      return;
    }
    renderSearchResults();
  };
  $("#tracker-go").addEventListener("click", runSearch);
  trackerInput.addEventListener("input", runSearch);
  trackerInput.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); runSearch(); }
    if (e.key === "Escape") { hideSearchResults(); trackerInput.blur(); }
  });
  $("#tracker-results").addEventListener("click", (e) => {
    const row = e.target.closest("[data-tracker-puuid]");
    if (row) {
      const p = findPlayer(row.dataset.trackerPuuid);
      if (p) selectTracker(p);
    }
  });
  document.addEventListener("click", (e) => {
    if (!e.target.closest(".tracker-search-wrap")) hideSearchResults();
  });

  /* aparência: preferências ficam no WebView (localStorage) */
  $("#set-accent").addEventListener("input", (e) => {
    applyCustomAccent(e.target.value);
    applyPrefs();
  });
  $("#set-accent-reset").addEventListener("click", () => {
    Prefs.accent = "";
    try { localStorage.removeItem("crymora-accent"); } catch (e) {}
    applyTheme(currentTheme());
  });
  $("#density-seg").addEventListener("click", (e) => {
    const b = e.target.closest("[data-density]");
    if (!b) return;
    Prefs.density = b.dataset.density === "compact" ? "compact" : "comfortable";
    savePref("density", Prefs.density);
    applyPrefs();
  });

  $("#view-settings").addEventListener("change", async (e) => {
    const id = e.target.id, on = e.target.checked;
    if (id === "set-boot") await Bridge.saveSettings({ launchOnBoot: on });
    if (id === "set-discord") await Bridge.saveSettings({ discordRpEnabled: on });
    if (id === "set-alerts") {
      await Bridge.saveSettings({ trackedInGameAlertsEnabled: on });
      Toasts.enabled = on;
    }
    if (id === "set-motion") { Prefs.motion = on; savePref("motion", on); applyPrefs(); }
    if (id === "set-bg") { Prefs.bg = on; savePref("bg", on); applyPrefs(); }
    if (id === "set-show-level") { Prefs.showLevel = on; savePref("showLevel", on); applyPrefs(); refreshProfileViews(); }
    if (id === "set-show-peak") { Prefs.showPeak = on; savePref("showPeak", on); applyPrefs(); refreshProfileViews(); }
    if (id === "set-show-lb") { Prefs.showLb = on; savePref("showLb", on); applyPrefs(); refreshProfileViews(); }
  });

  document.body.addEventListener("click", (e) => {
    const go = e.target.closest("[data-goto]");
    if (go) {
      switchTab(go.dataset.goto);
      if (go.dataset.focusSearch) setTimeout(() => $("#tracker-q")?.focus(), 80);
      return;
    }
    const b = e.target.closest("[data-open]");
    if (b) Bridge.openExternal(b.dataset.open);
  });

  // clique no card → modal de detalhes (botão Twitch/links externos não contam)
  $("#teams").addEventListener("click", (e) => {
    if (e.target.closest(".twitch, [data-open], .pm")) return;
    const card = e.target.closest(".pcard");
    if (card?.dataset.puuid) openDetails(card.dataset.puuid);
  });

  // clique no nome/linha do amigo → mesmo modal de estatísticas do roster
  $("#friends-list").addEventListener("click", (e) => {
    const row = e.target.closest(".frow[data-puuid]");
    if (row?.dataset.puuid) openDetails(row.dataset.puuid);
  });
  $("#friends-list").addEventListener("keydown", (e) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    const row = e.target.closest(".frow[data-puuid]");
    if (!row) return;
    e.preventDefault();
    openDetails(row.dataset.puuid);
  });

  // modal: fundo/× fecham; botão leva o jogador pra aba Tracker
  $("#modal").addEventListener("click", (e) => {
    if (e.target.closest("[data-close]")) return closeModal();
    const tr = e.target.closest("[data-tracker]");
    if (tr) {
      const p = findPlayer(State.modalPuuid);
      if (p) { State.trackerPlayer = p; State.trackerSig = null; }
      closeModal();
      switchTab("tracker");
    }
  });

  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("#modal").hidden) closeModal();
  });

  // ícone que falhar a carregar (agente não resolvido etc.) some da tela
  document.body.addEventListener("error", (e) => {
    if (e.target instanceof HTMLImageElement) e.target.classList.add("img-dead");
  }, true);

  switchTab("home");
  tick();
  tickFriends(); // Home já abre com a lista de amigos, sem esperar 5s
  setInterval(tick, 1000);
  setInterval(tickFriends, 3000);
  window.addEventListener("focus", refreshOnFocus);
  document.addEventListener("visibilitychange", refreshOnFocus);
}
/* ------------------------------------------------------------------ */
/* menu da bandeja — o Rust original abre uma janelinha com label
 * "tray-menu" carregando o MESMO index.html ao clicar com o direito no
 * ícone (evidência do bundle: TrayMenu com Open/Quit + toggle Discord).
 * Como essa janela recebe esta UI, renderizamos nela um menu compacto
 * em vez da aplicação inteira espremida.                            */
/* ------------------------------------------------------------------ */

async function bootTray() {
  document.title = "Crymora";
  applyPrefs();

  const s = await Bridge.settings();
  const rs = await Bridge.riotStatus();
  const on = !!rs?.clientRunning;

  document.body.innerHTML = `
    <div class="tray-root">
      <div class="tray-head">
        <span class="tray-logo">Crymora</span>
        <span class="tray-dot ${on ? "on" : ""}" title="${esc(on ? t("trayOn") : t("trayOff"))}"></span>
      </div>
      <p class="tray-sub">${esc(on ? t("trayOn") : t("trayOff"))}${rs?.selfName ? ` · ${esc(rs.selfName)}` : ""}</p>
      <label class="set">
        <span>${esc(t("setDiscord"))}</span>
        <input type="checkbox" id="tray-discord" ${s.discordRpEnabled ? "checked" : ""} />
        <i class="switch" aria-hidden="true"></i>
      </label>
      <button class="tray-btn" id="tray-open">${esc(t("trayOpen"))}</button>
      <button class="tray-btn danger" id="tray-quit">${esc(t("ctxQuit"))}</button>
    </div>`;

  $("#tray-discord").addEventListener("change", (e) =>
    invoke("settings_set", { patch: { discordRpEnabled: e.target.checked } }).catch(() => {}));

  /* comandos que existem no binário original (e no rebuild) */
  $("#tray-open").addEventListener("click", () =>
    invoke("open_main_window", {}).catch(() => {}));
  $("#tray-quit").addEventListener("click", () =>
    invoke("quit_app", {}).catch(() => {}));
}

/* janela em que este bundle está rodando ("main" ou "tray-menu") */
const winLabel = (() => {
  try {
    return window.__TAURI_INTERNALS__.metadata?.currentWindow?.label ?? "main";
  } catch (e) { return "main"; }
})();

if (winLabel === "tray-menu") bootTray();
else boot();
