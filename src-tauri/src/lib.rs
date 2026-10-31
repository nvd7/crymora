/* Crymora — companheiro local do Valorant
 *
 * Backend reescrito do zero (a fase 1 era patch no exe original).
 * Toda a engenharia reversa que embasou este arquivo está no binário
 * original (strings .rdata + estrutura de crates), evidência colhida em
 * out/2026:
 *   lockfile  : %LOCALAPPDATA%\Riot Games\Riot Client\Config\lockfile
 *               (5 campos separados por ':', Basic riot:{senha} na
 *               API local https://127.0.0.1:{port} — cert self-signed)
 *   local     : /chat/v4/friends, /chat/v4/presences (campo `private`
 *               = JSON base64), /entitlements/v1/token
 *   região    : %LOCALAPPDATA%\VALORANT\Saved\Logs\ShooterGame.log
 *               regex glz-([a-z]+)-\d+\.([a-z]+)\.a\.pvp\.net
 *   versão    : https://valorant-api.com/v1/version (X-Riot-ClientVersion)
 *   pregame   : glz /pregame/v1/players/{puuid}, /pregame/v1/matches/{id}
 *   ao vivo   : glz /core-game/v1/players/{puuid}, /core-game/v1/matches/{id}
 *   mmr       : pd /mmr/v1/players/{puuid} + competitiveupdates (histórico)
 *               (cache em mmr-cache.json — mesmo nome do original)
 *   discord   : crate discord-rich-presence 0.2.5 (o mesmo do original)
 *
 * Conexões externas: SÓ Riot (local + glz/pd) e valorant-api.com (CDN
 * público de ícones). Nada de backend próprio, telemetria, store, stripe.
 */

mod discord_rp;
mod riot;
mod settings;

use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

#[derive(Clone, Default)]
pub struct Snapshot {
    pub match_state: Value,
    pub roster: Option<Value>,
    pub friends: Value,
    pub riot_status: Value,
}

pub type Shared = Arc<Mutex<Snapshot>>;

/* ------------------------------------------------------------------ */
/* debug.log — release é windows_subsystem (sem console): eprintln!
 * não aparece em lugar nenhum. Todo clog! grava também em
 * %APPDATA%\crymora\debug.log — é daí que sai o diagnóstico real.     */
/* ------------------------------------------------------------------ */

pub fn log_append(args: std::fmt::Arguments) {
    use std::io::Write as _;
    let Some(local) = std::env::var_os("APPDATA") else { return };
    let dir = std::path::PathBuf::from(local).join("crymora");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("debug.log");
    /* log sem rotação crescia para sempre; 2 MB é suficiente para
     * diagnóstico e mantém o arquivo anterior em debug.log.1 */
    if std::fs::metadata(&path).map(|m| m.len() > 2 * 1024 * 1024).unwrap_or(false) {
        let _ = std::fs::rename(&path, dir.join("debug.log.1"));
    }
    let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    else {
        return;
    };
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let _ = writeln!(f, "[{t}] {args}");
}

#[macro_export]
macro_rules! clog {
    ($($arg:tt)*) => {{
        eprintln!($($arg)*);
        $crate::log_append(format_args!($($arg)*));
    }};
}

/* ------------------------------------------------------------------ */
/* comandos IPC — mesmos nomes/contratos que a UI já fala            */
/* ------------------------------------------------------------------ */

#[tauri::command]
async fn get_match_state(shared: State<'_, Shared>) -> Result<Value, String> {
    Ok(shared.lock().await.match_state.clone())
}

/* diagnóstico da UI — window.onerror/unhandledrejection do webview e
 * contadores de polling caem aqui (console de release é invisível) */
#[tauri::command]
fn js_diag(message: String) {
    /* limita o tamanho: um erro gigante do webview não pode inflar o log */
    let msg: String = message.chars().take(2000).collect();
    clog!("[ui] {msg}");
}

#[tauri::command]
async fn get_pregame(shared: State<'_, Shared>) -> Result<Value, String> {
    let snap = shared.lock().await;
    match &snap.roster {
        Some(r) if snap.match_state.get("phase") == Some(&json!("PREGAME")) => Ok(r.clone()),
        _ => Err("{\"note\":\"not in pregame\"}".into()),
    }
}

#[tauri::command]
async fn get_live_roster(shared: State<'_, Shared>) -> Result<Value, String> {
    let snap = shared.lock().await;
    match &snap.roster {
        Some(r) if snap.match_state.get("phase") == Some(&json!("INGAME")) => Ok(r.clone()),
        _ => Err("{\"note\":\"not in a live match\"}".into()),
    }
}

#[tauri::command]
async fn get_friends(shared: State<'_, Shared>) -> Result<Value, String> {
    Ok(shared.lock().await.friends.clone())
}

#[tauri::command]
async fn riot_status(shared: State<'_, Shared>) -> Result<Value, String> {
    Ok(shared.lock().await.riot_status.clone())
}

/* Alt+Tab / volta da bandeja: roda um tick imediatamente e publica o
 * snapshot, em vez de esperar o próximo intervalo do poll_loop. */
#[tauri::command]
async fn refresh_now(
    poller: State<'_, Arc<Mutex<riot::state::Poller>>>,
    shared: State<'_, Shared>,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    let (snap, _) = poller.lock().await.tick().await;
    *shared.lock().await = snap;
    clog!(
        "[crymora] refresh_now (foco) · {}ms",
        started.elapsed().as_millis()
    );
    Ok(())
}

/* Só https e só os hosts que o app realmente usa. Mesmo com a página
 * comprometida, o máximo que sai é um perfil do tracker.gg ou um canal da
 * Twitch — nada de file:, cmd:, javascript: ou phishing arbitrário. */
fn parse_external(url: &str) -> Result<reqwest::Url, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| e.to_string())?;
    if parsed.scheme() != "https" || parsed.port().is_some() || !parsed.username().is_empty() {
        return Err("link não permitido".into());
    }
    let host = parsed.host_str().unwrap_or("").to_ascii_lowercase();
    let allowed = matches!(
        host.as_str(),
        "tracker.gg" | "www.tracker.gg" | "twitch.tv" | "www.twitch.tv" | "m.twitch.tv"
    );
    if !allowed {
        return Err(format!("host não permitido: {host}"));
    }
    Ok(parsed)
}

#[tauri::command]
async fn open_external(url: String, app: tauri::AppHandle) -> Result<(), String> {
    let parsed = parse_external(&url)?;
    app.opener()
        .open_url(parsed.as_str(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse_external;

    #[test]
    fn open_external_accepts_only_known_hosts() {
        assert!(parse_external("https://tracker.gg/valorant/profile/riot/a%23b/overview").is_ok());
        assert!(parse_external("https://www.twitch.tv/nvd7").is_ok());
        assert!(parse_external("http://tracker.gg/").is_err());
        assert!(parse_external("file:///C:/Windows/System32/calc.exe").is_err());
        assert!(parse_external("javascript:alert(1)").is_err());
        assert!(parse_external("https://tracker.gg.evil.com/").is_err());
        assert!(parse_external("https://evil.com/?h=tracker.gg").is_err());
        assert!(parse_external("https://user@tracker.gg/").is_err());
        assert!(parse_external("https://tracker.gg:8443/").is_err());
        assert!(parse_external("https://twitch.tv.evil.com/").is_err());
    }
}

/* encerra o processo de verdade (menu de contexto → "Sair do Crymora") */
#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/* mostra/foca a janela principal — mesmo nome do comando do binário
 * original (o "Open Ascend Companion" do TrayMenu deles) */
#[tauri::command]
fn open_main_window(app: tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

#[tauri::command]
async fn settings_get(app: tauri::AppHandle) -> Result<Value, String> {
    settings::get(&app)
}

#[tauri::command]
async fn settings_set(app: tauri::AppHandle, patch: Value) -> Result<(), String> {
    settings::set(&app, patch)
}

#[tauri::command]
async fn tracker_proxy(_method: String, _path: String, _body: Option<Value>, _bearer: Option<String>) -> Result<Value, String> {
    // fase 2: webview escondida que carrega o tracker.gg de verdade
    // (passa no Cloudflare legitimamente) e expõe o HTML praqui.
    Err("tracker transport not wired yet".into())
}

/* ------------------------------------------------------------------ */
/* boot                                                                */
/* ------------------------------------------------------------------ */

pub fn run() {
    /* panic em qualquer task (poll_loop incluso) era morte silenciosa —
     * agora fica registrado no debug.log antes de sumir */
    std::panic::set_hook(Box::new(|info| {
        eprintln!("PANIC: {info}");
        crate::log_append(format_args!("PANIC: {info}"));
    }));

    let shared: Shared = Arc::new(Mutex::new(Snapshot::default()));

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .manage(shared.clone())
        /* X esconde pra bandeja (comportamento do original — "hidden-to-
         * tray"); sair de verdade é pelo menu da bandeja / quit_app */
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            // bandeja: menu nativo com abrir/sair (botão esquerdo abre)
            let open = MenuItem::with_id(app, "open", "Abrir Crymora", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Sair do Crymora", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &quit])?;
            let mut tray = TrayIconBuilder::new()
                .tooltip("Crymora")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => app.exit(0),
                    _ => {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        let app = tray.app_handle();
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            let poller = Arc::new(Mutex::new(riot::state::Poller::new(app.handle())));
            app.manage(poller.clone());
            let handle = app.handle().clone();
            let shared = shared.clone();
            tauri::async_runtime::spawn(async move {
                riot::poll_loop(handle, shared, poller).await;
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            js_diag,
            get_match_state,
            get_pregame,
            get_live_roster,
            get_friends,
            riot_status,
            refresh_now,
            open_external,
            quit_app,
            open_main_window,
            settings_get,
            settings_set,
            tracker_proxy,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Crymora");
}

/* emite evento pro frontend (usado no loop de polling p/ toasts futuros) */
#[allow(dead_code)]
fn emit(handle: &tauri::AppHandle, event: &str, payload: Value) {
    let _ = handle.emit(event, payload);
}
