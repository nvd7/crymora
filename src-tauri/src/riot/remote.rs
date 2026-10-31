/* Camada HTTP — blueprint colhido por RE do binário original:
 *   API local  : https://127.0.0.1:{port} · Basic riot:{senha} · cert
 *                self-signed → aceitar inválido
 *                /entitlements/v1/token, /chat/v4/friends,
 *                /chat/v4/presences (campo `private` = JSON base64),
 *                /player-account/lookup/v2/namesets-for-puuids (POST,
 *                único caminho que resolve nick de perfil oculto)
 *   glz        : https://glz-{região}-1.{shard}.a.pvp.net
 *                /pregame/v1/players/{puuid}, /pregame/v1/matches/{id},
 *                /core-game/v1/players/{puuid}, /core-game/v1/matches/{id}
 *   pd         : https://pd.{shard}.a.pvp.net
 *                /mmr/v1/players/{puuid}, /name/v1/players/{ids}
 *   versão     : https://valorant-api.com/v1/version (CDN público)
 * Cabeçalhos remotos: Bearer + X-Riot-Entitlements-JWT +
 *   X-Riot-ClientVersion + X-Riot-ClientPlatform (JSON base64). */

use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use base64::Engine;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::LazyLock;
use std::time::Duration;

use super::region::Region;
use crate::clog;

pub const PLATFORM_JSON: &str = concat!(
    "{\"platformType\":\"PC\",\"platformOS\":\"Windows\",",
    "\"platformOSVersion\":\"10.0.19042\",\"platformChipset\":\"Unknown\"}"
);

/* clientes compartilhados (conexões reutilizadas entre ticks) */
static LOCAL: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .danger_accept_invalid_certs(true) // cert self-signed do client
        .no_proxy() // 127.0.0.1 nunca passa por proxy
        .timeout(Duration::from_secs(4))
        .build()
        .expect("reqwest local")
});
static REMOTE: LazyLock<reqwest::Client> = LazyLock::new(|| {
    reqwest::Client::builder()
        .no_proxy() // Riot/valorant-api direto — sem proxy do sistema
        .timeout(Duration::from_secs(8))
        .build()
        .expect("reqwest remote")
});

/* ------------------------------------------------------------------ */
/* API local                                                           */
/* ------------------------------------------------------------------ */

/// GET na API local. O lockfile é relido a cada chamada — porta/senha
/// mudam sempre que o Riot Client reinicia.
pub async fn local(path: &str) -> Result<Value, String> {
    let lf = super::lockfile::read().ok_or("Riot client not running")?;
    let url = format!("https://127.0.0.1:{}{}", lf.port, path);
    let auth = STANDARD.encode(format!("riot:{}", lf.password));
    LOCAL
        .get(&url)
        .header(reqwest::header::AUTHORIZATION, format!("Basic {auth}"))
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())
}

/// POST na API local (mesmo Basic riot:{senha}). O Riot Client expõe
/// serviços que o pd não tem — o namesets é o que revela nick oculto.
/// O lockfile pode rotacionar (porta/senha) entre a leitura e o request:
/// um 401/recusa de conexão ganha uma segunda tentativa com lockfile novo.
pub async fn local_post(path: &str, body: &Value) -> Result<Value, String> {
    let mut last = String::from("local request failed");
    for _ in 0..2 {
        let lf = super::lockfile::read().ok_or("Riot client not running")?;
        let url = format!("https://127.0.0.1:{}{}", lf.port, path);
        let auth = STANDARD.encode(format!("riot:{}", lf.password));
        match LOCAL
            .post(&url)
            .header(reqwest::header::AUTHORIZATION, format!("Basic {auth}"))
            .json(body)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => {
                return r.json().await.map_err(|e| e.to_string());
            }
            Ok(r) => {
                let status = r.status();
                last = format!("HTTP {status}");
                // 401 = lockfile velho; outros erros não melhoram repetindo
                if status != reqwest::StatusCode::UNAUTHORIZED {
                    break;
                }
            }
            Err(e) => last = e.to_string(),
        }
    }
    Err(last)
}

/// Nick real de cada puuid via Riot Client local. É o único caminho que
/// devolve o GameName de quem está com o perfil oculto (o pd
/// /name-service/v2 responde GameName vazio nesses casos). Resposta:
///   {"namesets":[{"puuid":"…","alias":{"gameName":"…","tagLine":"…"}}]}
pub async fn local_namesets(
    puuids: &[String],
) -> Result<HashMap<String, (String, String)>, String> {
    let body = json!({ "puuids": puuids });
    let v = local_post("/player-account/lookup/v2/namesets-for-puuids", &body).await?;
    let mut out = HashMap::new();
    for e in v["namesets"].as_array().into_iter().flatten() {
        let Some(id) = e["puuid"].as_str() else { continue };
        let mut name = e["alias"]["gameName"].as_str().unwrap_or("");
        let mut tag = e["alias"]["tagLine"].as_str().unwrap_or("");
        // o v1 responde em `gnt`; fallback defensivo de shape
        if name.is_empty() {
            name = e["gnt"]["gameName"].as_str().unwrap_or("");
            tag = e["gnt"]["tagLine"].as_str().unwrap_or(tag);
        }
        if !name.is_empty() {
            out.insert(id.to_string(), (name.to_string(), tag.to_string()));
        }
    }
    Ok(out)
}

/// Sessão local: token + puuid (subject do /entitlements/v1/token).
#[derive(Clone)]
pub struct Local {
    pub puuid: String,
    pub access_token: String,
    pub entitlements_token: String,
}

impl Local {
    pub async fn load() -> Result<Local, String> {
        let v = local("/entitlements/v1/token").await?;
        let access = v["accessToken"].as_str().ok_or("sem accessToken")?.to_string();
        let puuid = v["subject"]
            .as_str()
            .and_then(|s| if s.is_empty() { None } else { Some(s.to_string()) })
            .or_else(|| jwt_sub(&access))
            .ok_or("token sem subject")?;
        // API local devolve o entitlement em "token" (o nome
        // "entitlementsToken" é do fluxo OAuth remoto — fallback só na dúvida)
        let entitlements_token = v["token"]
            .as_str()
            .or_else(|| v["entitlementsToken"].as_str())
            .ok_or("sem entitlementsToken")?
            .to_string();
        Ok(Local {
            puuid,
            access_token: access,
            entitlements_token,
        })
    }
}

/// claim `sub` de um JWT (payload segment em base64url) — fallback de puuid
pub fn jwt_sub(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .or_else(|_| STANDARD.decode(payload.trim_end_matches('=')))
        .ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v["sub"].as_str().map(String::from)
}

/// campo `private` da presença = JSON base64 (sessionState, queueId,
/// mapKey, partyId, competitiveTier, accountLevel, scoreAlly/scoreEnemy…)
pub fn presence_private(p: &Value) -> Value {
    let Some(b64) = p["private"].as_str() else { return Value::Null };
    let trimmed = b64.trim_end_matches('=');
    let bytes = STANDARD
        .decode(trimmed)
        .or_else(|_| URL_SAFE_NO_PAD.decode(trimmed))
        .ok();
    bytes
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or(Value::Null)
}

/* Client ATUAL (2026) aninha os campos — confirmado em dump ao vivo:
 *   matchPresenceData: { sessionLoopState, queueId, matchMap, … }
 *   playerPresenceData: { competitiveTier, accountLevel, playerCardId, … }
 *   partyId/partySize/queueId na raiz
 * O formato antigo era plano (sessionState, mapKey, …) — priv_info
 * cobre os dois, priorizando o novo. Null em tudo o que não existir. */
#[derive(Clone, Debug, Default)]
pub struct Priv {
    pub session_state: String,
    pub queue_id: String,
    pub map_key: String,
    pub tier: i64,
    pub level: i64,
    pub card: String,
    pub party_id: String,
    pub score_ally: Option<i64>,
    pub score_enemy: Option<i64>,
}

pub fn priv_info(private: &Value) -> Priv {
    let m = &private["matchPresenceData"];
    let p = &private["playerPresenceData"];
    Priv {
        session_state: m["sessionLoopState"]
            .as_str()
            .or_else(|| private["sessionState"].as_str())
            .unwrap_or("")
            .to_string(),
        queue_id: m["queueId"]
            .as_str()
            .or_else(|| private["queueId"].as_str())
            .unwrap_or("")
            .to_string(),
        map_key: m["matchMap"]
            .as_str()
            .or_else(|| private["mapKey"].as_str())
            .unwrap_or("")
            .to_string(),
        tier: p["competitiveTier"]
            .as_i64()
            .or_else(|| private["competitiveTier"].as_i64())
            .unwrap_or(0),
        level: p["accountLevel"]
            .as_i64()
            .or_else(|| private["accountLevel"].as_i64())
            .unwrap_or(0),
        card: p["playerCardId"]
            .as_str()
            .or_else(|| private["playerCardId"].as_str())
            .unwrap_or("")
            .to_string(),
        party_id: private["partyId"].as_str().unwrap_or("").to_string(),
        score_ally: private["scoreAlly"]
            .as_i64()
            .or_else(|| m["currentAllyTeamScore"].as_i64())
            .or_else(|| private["partyOwnerMatchScoreAllyTeam"].as_i64()),
        score_enemy: private["scoreEnemy"]
            .as_i64()
            .or_else(|| m["currentEnemyTeamScore"].as_i64())
            .or_else(|| private["partyOwnerMatchScoreEnemyTeam"].as_i64()),
    }
}

/// presença (payload cru) → campos do `private` já normalizados
pub fn presence_priv_info(p: &Value) -> Priv {
    priv_info(&presence_private(p))
}

/// Presença 2026: `state` substituiu `availability` ("chat", "dnd",
/// "away", "mobile"…). Vazio = sem presença/offline.
pub fn presence_state(p: &Value) -> String {
    p["state"].as_str().unwrap_or("").to_string()
}

/// `packedData` (base64) traz o tamanho REAL da party — membros que não
/// são nossos amigos não têm presença, então contar entradas subestima.
pub fn presence_party_size(p: &Value) -> Option<i64> {
    let b64 = p["packedData"].as_str()?;
    let trimmed = b64.trim_end_matches('=');
    let bytes = STANDARD
        .decode(trimmed)
        .or_else(|_| URL_SAFE_NO_PAD.decode(trimmed))
        .ok()?;
    let v: Value = serde_json::from_slice(&bytes).ok()?;
    v["party"]["currentSize"].as_i64().filter(|n| *n > 0)
}

/* ------------------------------------------------------------------ */
/* sessão remota (glz/pd + versão do client)                           */
/* ------------------------------------------------------------------ */

pub struct Ctx {
    pub region: Region,
    pub local: Local,
    pub client_version: String,
}

impl Ctx {
    pub fn new(region: Region, local: Local, client_version: String) -> Self {
        Self { region, local, client_version }
    }

    fn authed(&self, method: reqwest::Method, url: &str) -> reqwest::RequestBuilder {
        REMOTE
            .request(method, url)
            .header(
                reqwest::header::AUTHORIZATION,
                format!("Bearer {}", self.local.access_token),
            )
            .header("X-Riot-Entitlements-JWT", &self.local.entitlements_token)
            .header("X-Riot-ClientVersion", &self.client_version)
            .header("X-Riot-ClientPlatform", STANDARD.encode(PLATFORM_JSON.as_bytes()))
    }

    pub async fn glz(&self, path: &str) -> Result<Value, String> {
        let url = format!(
            "https://glz-{}-1.{}.a.pvp.net{}",
            self.region.region, self.region.shard, path
        );
        self.authed(reqwest::Method::GET, &url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }

    pub async fn pd(&self, path: &str) -> Result<Value, String> {
        let url = format!("https://pd.{}.a.pvp.net{}", self.region.shard, path);
        self.authed(reqwest::Method::GET, &url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }

    /// Pedido autenticado com corpo no pd — o name-service é PUT
    /// (POST devolve 405 Method Not Allowed)
    async fn pd_send(&self, method: reqwest::Method, path: &str, body: &Value) -> Result<Value, String> {
        let url = format!("https://pd.{}.a.pvp.net{}", self.region.shard, path);
        self.authed(method, &url)
            .json(body)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())
    }

    /// Riot IDs em lote. Ordem:
    ///   1. namesets do Riot Client local — resolve inclusive perfil oculto
    ///   2. pd /name-service/v2/players — fallback p/ o que faltar
    pub async fn names(&self, puuids: &[String]) -> Result<HashMap<String, (String, String)>, String> {
        let mut out = HashMap::new();
        if puuids.is_empty() {
            return Ok(out);
        }
        let body_of = |ids: &[String]| Value::Array(ids.iter().cloned().map(Value::String).collect());

        /* 1) API local: o pd devolve GameName vazio para quem está
         * "Oculto"; o namesets do client devolve o nick real. */
        match local_namesets(puuids).await {
            Ok(map) => {
                clog!("[crymora] namesets local: {}/{}", map.len(), puuids.len());
                out.extend(map);
            }
            Err(e) => clog!("[crymora] namesets local falhou: {e}"),
        }

        /* 2) pd só para quem o client local não devolveu (ex.: puuid
         * recém-chegado que o cache local ainda não conhece). */
        let missing: Vec<String> = puuids
            .iter()
            .filter(|id| !out.contains_key(*id))
            .cloned()
            .collect();
        if !missing.is_empty() {
            match self
                .pd_send(reqwest::Method::PUT, "/name-service/v2/players", &body_of(&missing))
                .await
            {
                Ok(v) => collect_names(&v, &mut out),
                Err(e) => {
                    if out.is_empty() {
                        return Err(e);
                    }
                    clog!("[crymora] name-service pd falhou: {e}");
                }
            }
        }
        clog!("[crymora] names: {}/{} resolvidos", out.len(), puuids.len());
        Ok(out)
    }
}

/// extrai (GameName, TagLine) de mapa {puuid: {...}} ou array [{Subject…}]
fn collect_names(v: &Value, out: &mut HashMap<String, (String, String)>) {
    let mut put = |k: &str, e: &Value| {
        let name = e["GameName"]
            .as_str()
            .or_else(|| e["gameName"].as_str())
            .unwrap_or_default();
        let tag = e["TagLine"]
            .as_str()
            .or_else(|| e["tagLine"].as_str())
            .unwrap_or_default();
        if !k.is_empty() && !name.is_empty() {
            out.insert(k.to_string(), (name.to_string(), tag.to_string()));
        }
    };
    match v {
        Value::Object(map) => {
            for (k, e) in map {
                put(k, e);
            }
        }
        Value::Array(arr) => {
            for e in arr {
                let k = e["Subject"]
                    .as_str()
                    .or_else(|| e["puuid"].as_str())
                    .unwrap_or_default();
                put(k, e);
            }
        }
        _ => {}
    }
}

/// X-Riot-ClientVersion. O pd espera o formato "release-13.08-shipping-22-…"
/// (a linha "CI server version" do ShooterGame.log é exatamente a que o jogo
/// manda); mandar o campo `version` cru ("13.08.00.5715960") faz o pd
/// responder 500 INTERNAL_UNHANDLED_SERVER_ERROR. Ordem: log do jogo →
/// valorant-api (`riotClientVersion`, a mesma chave que o original usava).
pub async fn client_version() -> String {
    if let Some(v) = super::region::client_version_from_log() {
        return v;
    }
    static CACHED: tokio::sync::OnceCell<String> = tokio::sync::OnceCell::const_new();
    CACHED
        .get_or_init(|| async {
            match REMOTE.get("https://valorant-api.com/v1/version").send().await {
                Ok(r) => r
                    .json::<Value>()
                    .await
                    .ok()
                    .and_then(|v| {
                        v["data"]["riotClientVersion"]
                            .as_str()
                            .or_else(|| v["data"]["version"].as_str())
                            .map(String::from)
                    })
                    .unwrap_or_else(|| "release-1.0".into()),
                Err(_) => "release-1.0".into(),
            }
        })
        .await
        .clone()
}

/* ------------------------------------------------------------------ */
/* MMR — pd /mmr/v1/players/{puuid} (cache no state.rs)                */
/* ------------------------------------------------------------------ */

#[derive(Clone, Debug, Default)]
pub struct MmrInfo {
    pub tier: i64,
    pub rr: i64,
    pub peak: i64,
    pub act_peak: Option<i64>,
    pub act_wins: i64,
    pub act_games: i64,
    pub prev: Option<(i64, i64, i64)>, // (tier, wins, games)
    pub career_games: i64,
    pub leaderboard: Option<i64>,
}

pub async fn mmr(ctx: &Ctx, puuid: &str) -> Result<MmrInfo, String> {
    let v = ctx
        .pd(&format!("/mmr/v1/players/{}", puuid))
        .await?;
    let seasons = seasons_desc().await;
    let info = parse_mmr(&v, &seasons);
    clog!(
        "[crymora] mmr {}…: tier={} rr={} peak={} seasons={}",
        puuid.chars().take(8).collect::<String>(),
        info.tier, info.rr, info.peak, seasons.len()
    );
    Ok(info)
}

/// SeasonalInfoBySeasonID é um mapa por UUID — a ordem cronológica vem da
/// lista de atos do valorant-api (mesma família do CDN de ícones).
pub fn parse_mmr(v: &Value, seasons: &[(String, i64)]) -> MmrInfo {
    /* Formato 2026 (dump ao vivo): não existe mais "Current" — o rank
     * atual fica em QueueSkills.competitive.SeasonalInfoBySeasonID, uma
     * entrada por ato: {CompetitiveTier, RankedRating, NumberOfWins,
     * NumberOfGames, LeaderboardRank, Rank, WinsByTier…}. */
    let mut info = MmrInfo::default();
    let Some(map) = v["QueueSkills"]["competitive"]["SeasonalInfoBySeasonID"].as_object() else {
        return info;
    };

    /* carreira: pico de tier e total de partidas entre os atos jogados.
     * `CompetitiveTier` é o tier no FIM do ato — quem chegou a Radiant e
     * caiu para Immortal 2 fecharia o ato com 24. O pico real fica em
     * `Rank` (maior tier alcançado no ato; ex.: 27 = Radiant). Atos sem
     * `Rank` (0) caem no `CompetitiveTier`. Confere com o tracker.gg:
     * temporada com tier 24 / rank 27 → pico Radiant. */
    for s in map.values() {
        let end_tier = s["CompetitiveTier"].as_i64().unwrap_or(0);
        let act_peak = s["Rank"].as_i64().filter(|r| *r > 0).unwrap_or(end_tier);
        if act_peak > info.peak {
            info.peak = act_peak;
        }
        info.career_games += s["NumberOfGames"].as_i64().unwrap_or(0);
    }

    /* entradas com dados, na ordem cronológica (mais recente primeiro) */
    let now = unix_now();
    let cur_idx = seasons
        .iter()
        .position(|(_, st)| *st <= now)
        .unwrap_or(0);
    let entries: Vec<(usize, &Value)> = seasons
        .iter()
        .enumerate()
        .filter_map(|(i, (id, _))| map.get(id).map(|e| (i, e)))
        .collect();

    // ato vigente = primeira entrada dele em diante; sem entrada ainda
    // (começo de ato) → cai na mais recente jogada
    let act_pos = entries
        .iter()
        .position(|(i, _)| *i >= cur_idx)
        .map(|p| p)
        .unwrap_or(0);
    if let Some((_, e)) = entries.get(act_pos) {
        info.tier = e["CompetitiveTier"].as_i64().unwrap_or(0);
        info.rr = e["RankedRating"].as_i64().unwrap_or(0);
        info.leaderboard = e["LeaderboardRank"].as_i64().filter(|l| *l > 0);
        info.act_peak = e["Rank"]
            .as_i64()
            .filter(|r| *r > 0)
            .or_else(|| e["CompetitiveTier"].as_i64());
        info.act_wins = e["NumberOfWins"].as_i64().unwrap_or(0);
        info.act_games = e["NumberOfGames"].as_i64().unwrap_or(0);
        if let Some((_, prev)) = entries.get(act_pos + 1) {
            info.prev = Some((
                prev["CompetitiveTier"].as_i64().unwrap_or(0),
                prev["NumberOfWins"].as_i64().unwrap_or(0),
                prev["NumberOfGames"].as_i64().unwrap_or(0),
            ));
        }
    }
    info
}

/// atos competitivos do CDN público: [(uuid, início unix-seg)] desc.
/// Nada de chrono — as datas ISO-8601 Z ordenam lexicograficamente.
async fn seasons_desc() -> Vec<(String, i64)> {
    static SEASONS: tokio::sync::OnceCell<Vec<(String, i64)>> =
        tokio::sync::OnceCell::const_new();
    SEASONS
        .get_or_init(|| async {
            let mut out: Vec<(String, i64)> = Vec::new();
            if let Ok(r) = REMOTE.get("https://valorant-api.com/v1/seasons").send().await {
                if let Ok(v) = r.json::<Value>().await {
                    out = v["data"]
                        .as_array()
                        .map(|arr| {
                            arr.iter()
                                .filter(|s| {
                                    // type é "EAresSeasonType::Act" (episódios: null)
                                    s["type"]
                                        .as_str()
                                        .map(|t| t.contains("Act"))
                                        .unwrap_or(false)
                                })
                                .filter_map(|s| {
                                    let id = s["uuid"].as_str()?.to_string();
                                    let st = iso_to_unix(s["startTime"].as_str()?)?;
                                    Some((id, st))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                }
            }
            out.sort_by(|a, b| b.1.cmp(&a.1));
            out
        })
        .await
        .clone()
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// "2026-10-23T12:34:56Z" → unix-seg (algoritmo civil de H. Hinnant)
fn iso_to_unix(s: &str) -> Option<i64> {
    let s = s.trim_end_matches('Z');
    let (y, rest) = s.split_once('-')?;
    let (m, rest) = rest.split_once('-')?;
    let (d, rest) = rest.split_once('T')?;
    let (hh, rest) = rest.split_once(':')?;
    let (mm, ss) = rest.split_once(':')?;
    let (y, m, d): (i64, i64, i64) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    let days = days_from_civil(y, m, d);
    Some(days * 86400 + hh.parse::<i64>().ok()? * 3600 + mm.parse::<i64>().ok()? * 60 + ss.parse::<i64>().ok()?)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/* ------------------------------------------------------------------ */
/* util                                                                */
/* ------------------------------------------------------------------ */

/// caminho de UE ("/Game/Maps/Ascent/Ascent") → último segmento
pub fn last_seg(s: &str) -> String {
    s.rsplit('/').next().unwrap_or(s).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn iso_parse() {
        assert_eq!(iso_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(iso_to_unix("2026-01-10T12:00:00Z"), Some(1768046400));
    }

    #[test]
    fn mmr_two_seasons() {
        let v = json!({
            "QueueSkills": {"competitive": {"SeasonalInfoBySeasonID": {
                "aa": {"CompetitiveTier": 19, "NumberOfWins": 10, "NumberOfGames": 20, "Rank": 20},
                "zz": {"CompetitiveTier": 21, "NumberOfWins": 30, "NumberOfGames": 50,
                       "Rank": 22, "RankedRating": 55, "LeaderboardRank": 0}
            }}}
        });
        let seasons = vec![("zz".into(), 1700000000i64), ("aa".into(), 1600000000)];
        let m = parse_mmr(&v, &seasons);
        assert_eq!(m.tier, 21);
        assert_eq!(m.rr, 55);
        assert_eq!(m.peak, 22); // Rank (pico do ato) vence o tier de fim
        assert_eq!(m.career_games, 70);
        assert_eq!(m.act_wins, 30); // ato mais recente
        assert_eq!(m.prev.unwrap().0, 19);
        assert_eq!(m.act_peak, Some(22));
    }

    /* caso real (tracker.gg): ato terminou em Immortal 2 (24), mas o
     * jogador chegou a Radiant (27) no mesmo ato — o pico é Radiant. */
    #[test]
    fn mmr_peak_uses_highest_rank_of_act() {
        let v = json!({
            "QueueSkills": {"competitive": {"SeasonalInfoBySeasonID": {
                "radiant-act": {
                    "CompetitiveTier": 24, "Rank": 27, "RankedRating": 364,
                    "NumberOfWins": 70, "NumberOfGames": 115
                },
                "older-act": {
                    "CompetitiveTier": 19, "Rank": 0, "RankedRating": 90,
                    "NumberOfWins": 6, "NumberOfGames": 30
                }
            }}}
        });
        let seasons = vec![
            ("radiant-act".into(), 1700000000i64),
            ("older-act".into(), 1600000000i64),
        ];
        let m = parse_mmr(&v, &seasons);
        assert_eq!(m.tier, 24); // rank atual = fim do ato
        assert_eq!(m.peak, 27); // pico de carreira = Radiant
        assert_eq!(m.act_peak, Some(27));
        assert_eq!(m.career_games, 145);
    }

    /* teste manual (não roda no CI): confirma que o Riot Client local
     * devolve o nick real, inclusive de perfil oculto.
     *   $env:NAMESETS_TEST_PUUIDS="uuid1,uuid2,…"
     *   cargo test --lib local_namesets_live -- --ignored --nocapture */
    #[tokio::test]
    #[ignore = "live: exige Riot Client aberto e NAMESETS_TEST_PUUIDS"]
    async fn local_namesets_live() {
        let Ok(raw) = std::env::var("NAMESETS_TEST_PUUIDS") else {
            return;
        };
        let ids: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        assert!(!ids.is_empty());
        let m = local_namesets(&ids).await.expect("namesets local");
        for (id, (name, tag)) in &m {
            println!("{id} -> {name}#{tag}");
        }
        assert_eq!(m.len(), ids.len(), "faltou resolver algum puuid");
    }
}
