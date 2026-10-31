/* settings.json — mesmas chaves que a UI já fala (compat com a
 * edição patcheada, só que em diretório próprio do Crymora) */

use serde_json::{json, Map, Value};
use std::fs;
use std::path::PathBuf;

use tauri::Manager;

fn path(app: &tauri::AppHandle) -> PathBuf {
    let dir = app.path().app_config_dir().unwrap_or_else(|_| {
        PathBuf::from(&std::env::var("APPDATA").unwrap_or_default()).join("crymora")
    });
    let _ = fs::create_dir_all(&dir);
    dir.join("settings.json")
}

fn known(k: &str) -> bool {
    matches!(
        k,
        "launchOnBoot" | "trackedInGameAlertsEnabled" | "discordRpEnabled" | "telemetryOptIn"
    )
}

pub fn get(app: &tauri::AppHandle) -> Result<Value, String> {
    let p = path(app);
    let defaults = || json!({
        "launchOnBoot": false,
        "trackedInGameAlertsEnabled": true,
        "discordRpEnabled": false,
        "telemetryOptIn": false,
    });
    match fs::read_to_string(&p) {
        Ok(txt) => {
            let mut out = defaults();
            /* sobrescreve só chaves conhecidas e booleanas; descarta lixo
             * antigo (ex.: pregameEnemyRevealOptIn, removido por segurança) */
            if let Ok(Value::Object(obj)) = serde_json::from_str::<Value>(&txt) {
                let dst = out.as_object_mut().expect("defaults é objeto");
                for (k, v) in obj {
                    if known(&k) && v.is_boolean() {
                        dst.insert(k, v);
                    }
                }
            }
            Ok(out)
        }
        Err(_) => Ok(defaults()),
    }
}

pub fn set(app: &tauri::AppHandle, patch: Value) -> Result<(), String> {
    let p = path(app);
    let mut cur = get(app)?.as_object().cloned().unwrap_or(Map::new());
    if let Some(obj) = patch.as_object() {
        for (k, v) in obj {
            /* whitelist: a UI só alterna booleanos conhecidos; qualquer
             * outra chave/valor é ignorada para não virar lixo no JSON */
            if known(k) && v.is_boolean() {
                cur.insert(k.clone(), v.clone());
            }
        }
    }
    let tmp = p.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&cur).unwrap()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &p).map_err(|e| e.to_string())?;
    Ok(())
}
