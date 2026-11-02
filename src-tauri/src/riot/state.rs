/* poll_loop — o coração do rebuild: a cada ~3s monta um Snapshot completo
 * (o contrato exato que a UI já fala) e alimenta a presence do Discord.
 * Ordem herdada do original (evidência .rdata):
 *   lockfile → entitlements → região (ShooterGame.log) → presença
 *   própria (phase) → pregame/core-game → roster enriquecido com MMR
 *   (mmr-cache.json) → friends (presences) → Snapshot + Discord. */

use futures_util::future::join_all;
use serde_json::{json, Map, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use super::lockfile;
use super::region::{self, Region};
use super::remote::{self, Ctx, Local, MmrInfo};
use crate::discord_rp::DiscordRp;
use crate::Snapshot;
use crate::clog;

/* Intervalo do poll: mais curto durante partida (placar/roster mudam) e
 * mais folgado no menu. O foco da janela roda um tick na hora — quem
 * volta com Alt+Tab não espera este intervalo. */
const TICK_IDLE_MS: u64 = 1500;
const TICK_ACTIVE_MS: u64 = 1000;
const MMR_TTL_SECS: i64 = 600; // 10 min — mesmo espírito do cache original
const LOG_FRESH_SECS: u64 = 180; // ShooterGame.log escrito há pouco
/* Pré-jogo: espera os elos de todos antes de revelar a partida. Se o pd
 * recusar alguma consulta, libera a tela com o que veio. */
const RANK_WAIT_SECS: i64 = 5;

/* Próprio grupo: o payload da partida não traz PartyID (o Riot removeu),
 * então a API de party do glz é consultada com TTL. 20 s acompanha
 * entrada/saída de membros sem 2 requests a cada tick. */
const PARTY_TTL_SECS: i64 = 20;
const PARTY_RETRY_SECS: i64 = 30;

pub struct RpInfo {
    pub phase: String,
    pub details: String,
    pub state: String,
}

/* ------------------------------------------------------------------ */
/* caches                                                              */
/* ------------------------------------------------------------------ */

struct Caches {
    /// mmr-cache.json — { puuid: { ts, tier, rr, … } } (mesmo nome do original)
    mmr: Map<String, Value>,
    /// Riot IDs via namesets do Riot Client local (+ pd v2 de fallback)
    names: HashMap<String, (String, String)>,
    /// friends-cache.json — último tier/level/card visto por amigo
    /// (presença de quem tá offline não carrega esses campos)
    friends: Map<String, Value>,
    /// backoffs p/ não metralhar o pd quando ele recusa (429): puuid → retry
    mmr_backoff: HashMap<String, i64>,
    names_backoff: i64,
    self_name_tried: bool,
    /// último puuid próprio visto — detecta troca de conta mesmo quando a
    /// sessão anterior cai para `None` no meio da troca
    last_self_puuid: Option<String>,
    /// primeiro tick em PREGAME e se a partida já foi revelada — a UI só
    /// vê o pré-jogo depois que todos os elos resolvem (ou no timeout)
    pregame_since: Option<i64>,
    pregame_shown: bool,
    /// próprio grupo: (partyId, membros, timestamp). O Riot não manda
    /// `PartyID` no payload da partida; a API de party é a fonte ao vivo.
    /// TTL curto para acompanhar entrada/saída sem 2 requests por tick.
    self_party: Option<(String, Vec<String>, i64)>,
    /// falhou ao consultar a party? não tenta de novo antes disso
    party_backoff: i64,
}

fn config_dir(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from(&std::env::var("APPDATA").unwrap_or_default()).join("crymora"));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn load_json_map(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default()
}

fn save_json_map(path: &Path, map: &Map<String, Value>) {
    let tmp = path.with_extension("json.tmp");
    if let Ok(t) = serde_json::to_string(map) {
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// ShooterGame.log recentemente escrito → o jogo está de pé
fn shooter_log_fresh() -> bool {
    let Ok(local) = std::env::var("LOCALAPPDATA") else { return false };
    let p = PathBuf::from(local)
        .join("VALORANT").join("Saved").join("Logs").join("ShooterGame.log");
    std::fs::metadata(&p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|d| d.as_secs() < LOG_FRESH_SECS)
        .unwrap_or(false)
}

/* ------------------------------------------------------------------ */
/* MMR (mmr-cache.json)                                                */
/* ------------------------------------------------------------------ */

/* versão do cache: subir quando a interpretação do payload do pd mudar.
 * v2 = pico de carreira por `Rank` (maior tier do ato), não pelo tier de
 * fim de ato. Entradas antigas são refeitas no próximo tick. */
const MMR_CACHE_VERSION: i64 = 2;

fn mmr_cache_ok(e: &Value, now: i64) -> bool {
    e["v"].as_i64() == Some(MMR_CACHE_VERSION)
        && now - e["ts"].as_i64().unwrap_or(0) < MMR_TTL_SECS
}

fn mmr_entry(m: &MmrInfo, ts: i64) -> Value {
    json!({
        "v": MMR_CACHE_VERSION,
        "ts": ts,
        "tier": m.tier, "rr": m.rr, "peak": m.peak,
        "actPeak": m.act_peak, "actWins": m.act_wins, "actGames": m.act_games,
        "prevTier": m.prev.map(|p| p.0),
        "prevWins": m.prev.map(|p| p.1),
        "prevGames": m.prev.map(|p| p.2),
        "career": m.career_games, "lb": m.leaderboard,
    })
}

fn mmr_decode(e: &Value) -> MmrInfo {
    MmrInfo {
        tier: e["tier"].as_i64().unwrap_or(0),
        rr: e["rr"].as_i64().unwrap_or(0),
        peak: e["peak"].as_i64().unwrap_or(0),
        act_peak: e["actPeak"].as_i64(),
        act_wins: e["actWins"].as_i64().unwrap_or(0),
        act_games: e["actGames"].as_i64().unwrap_or(0),
        prev: match (e["prevTier"].as_i64(), e["prevWins"].as_i64(), e["prevGames"].as_i64()) {
            (Some(t), Some(w), Some(g)) => Some((t, w, g)),
            _ => None,
        },
        career_games: e["career"].as_i64().unwrap_or(0),
        leaderboard: e["lb"].as_i64(),
    }
}

/// Só vale gastar o orçamento do tick quando há de fato uma consulta a
/// fazer. Entrar em cache-fresco/backoff não pode consumir cota — era o
/// que impedia os jogadores do fim do roster de buscar MMR.
fn mmr_needs_fetch(cache: &Caches, puuid: &str, now: i64) -> bool {
    let fresh = cache
        .mmr
        .get(puuid)
        .map(|e| mmr_cache_ok(e, now))
        .unwrap_or(false);
    let backoff = cache.mmr_backoff.get(puuid).copied().unwrap_or(0) > now;
    !fresh && !backoff
}

/// Busca o MMR de vários jogadores em paralelo. O pd é local e aguenta
/// um burst do tamanho de um roster; em troca o pré-jogo resolve todos
/// os ranks no primeiro tick em vez de 4 por tick.
async fn mmr_batch(ctx: &Ctx, puuids: &[String]) -> Vec<Result<MmrInfo, String>> {
    join_all(puuids.iter().map(|id| remote::mmr(ctx, id))).await
}

/// Aplica o resultado no cache (com backoff de 90s em erro) e devolve o
/// MMR que a UI deve mostrar.
fn mmr_store(
    cache: &mut Caches,
    puuid: &str,
    res: Result<MmrInfo, String>,
    now: i64,
) -> Option<MmrInfo> {
    match res {
        Ok(m) => {
            cache.mmr_backoff.remove(puuid);
            cache.mmr.insert(puuid.to_string(), mmr_entry(&m, now));
            Some(m)
        }
        Err(e) => {
            clog!("[crymora] mmr erro ({puuid}): {e} — backoff 90s");
            cache.mmr_backoff.insert(puuid.to_string(), now + 90);
            // cache antigo (v1) tem pico errado — melhor não mostrar
            cache
                .mmr
                .get(puuid)
                .filter(|e| e["v"].as_i64() == Some(MMR_CACHE_VERSION))
                .map(mmr_decode)
        }
    }
}

/* ------------------------------------------------------------------ */
/* helpers de payload                                                  */
/* ------------------------------------------------------------------ */

/// primeiro campo existente (os payloads variam entre pregame/core-game)
fn s<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| v[*k].as_str())
}

/// puuid do jogador. No pré-jogo o `Subject` de cima pode vir vazio/nulo
/// para os inimigos (anti-scouting); o uuid real continua em
/// `PlayerIdentity.Subject`. Sem esse fallback o time inimigo sumia.
fn player_puuid(v: &Value) -> Option<&str> {
    let direct = s(v, &["puuid", "Subject", "AccountID"]);
    let nested = v["PlayerIdentity"]["Subject"]
        .as_str()
        .or_else(|| v["PlayerIdentity"]["subject"].as_str());
    direct.into_iter().chain(nested).find(|id| !id.is_empty())
}

struct RawPlayer {
    puuid: String,
    name: Option<String>,
    tag: Option<String>,
    agent: String,
    team: String,
    party: String,
    level: i64,
    /// tier vindo do próprio payload do pregame (o glz entrega na
    /// agent-select — ranque instantâneo sem esperar o pd; core-game
    /// não traz, aí vale 0 e o MMR do pd assume)
    tier_hint: i64,
}

fn build_player(p: &RawPlayer, m: Option<&MmrInfo>, is_self: bool) -> Value {
    json!({
        "puuid": p.puuid,
        "name": p.name.clone().unwrap_or_default(),
        "tagLine": p.tag.clone().unwrap_or_default(),
        "agent": p.agent,
        "teamId": p.team,
        "partyId": p.party,
        "accountLevel": p.level,
        "isSelf": is_self,
        "competitiveTier": m.map(|m| m.tier).unwrap_or(p.tier_hint),
        "rankResolved": m.is_some(),
        "rank": m.map(|m| json!({ "tier": m.tier, "rr": m.rr })),
        "peakTier": m.map(|m| m.peak),
        "peakResolved": m.map(|_| true).unwrap_or(false),
        "actPeakTier": m.and_then(|m| m.act_peak),
        "leaderboardRank": m.and_then(|m| m.leaderboard),
        "actWins": m.map(|m| m.act_wins).unwrap_or(0),
        "actGames": m.map(|m| m.act_games).unwrap_or(0),
        "prevAct": m.and_then(|m| m.prev.map(|(t, w, g)| json!({
            "tier": t, "wins": w, "games": g
        }))),
        "careerGames": m.map(|m| m.career_games).unwrap_or(0),
        "form": [], // TODO fase 2: match-history do pd (a UI degrada bem sem isso)
    })
}

/* ------------------------------------------------------------------ */
/* roster (pregame / core-game)                                        */
/* ------------------------------------------------------------------ */

/// No pré-jogo o payload do glz entrega só o time aliado (`EnemyTeam`
/// nulo). A fonte primária dos inimigos é o endpoint de loadouts (que
/// devolve o Subject dos 10); o ShooterGame.log é a reserva, pois os
/// inimigos só aparecem nele no fim do agent-select. Sem isso a UI
/// ficava com 5/5 e a seção "Inimigos" vazia.
fn parse_enemy_puuids(
    text: &str,
    match_id: &str,
    allies: &HashSet<&str>,
    self_puuid: &str,
    max: usize,
) -> Vec<String> {
    // primeira consulta do pré-jogo para ESTA partida; os inimigos só
    // aparecem no log no fim do agent-select, então precisamos varrer
    // desde o começo (a última consulta mudaria a âncora a cada tick).
    let lines: Vec<&str> = text.lines().collect();
    let Some(anchor) = lines
        .iter()
        .position(|line| line.contains(match_id) && line.contains("Pregame_GetMatch"))
    else {
        return vec![];
    };

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in lines[anchor + 1..].iter().copied() {
        // nova partida = para de ler o log da anterior
        if line.contains("Pregame_GetMatch") && !line.contains(match_id) {
            break;
        }
        let Some(pos) = line.find("Subject had empty display name. ") else { continue };
        let rest = &line[pos + "Subject had empty display name. ".len()..];
        let id = rest.get(..36).unwrap_or("");
        if id.len() != 36 || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
            continue;
        }
        if id == self_puuid || allies.contains(id) || !seen.insert(id.to_string()) {
            continue;
        }
        out.push(id.to_string());
        if out.len() >= max {
            break;
        }
    }
    out
}

fn enemies_from_log(
    match_id: &str,
    allies: &[RawPlayer],
    self_puuid: &str,
    max: usize,
) -> Vec<RawPlayer> {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(local) = std::env::var("LOCALAPPDATA") else { return vec![] };
    let path = PathBuf::from(local)
        .join("VALORANT").join("Saved").join("Logs").join("ShooterGame.log");
    let Ok(mut file) = std::fs::File::open(&path) else { return vec![] };
    let Ok(meta) = file.metadata() else { return vec![] };
    // log de sessão longa pode ter dezenas de MB: 2 MB do fim bastam
    let start = meta.len().saturating_sub(2 * 1024 * 1024);
    if file.seek(SeekFrom::Start(start)).is_err() {
        return vec![];
    }
    let mut buf = Vec::new();
    if file.read_to_end(&mut buf).is_err() {
        return vec![];
    }
    let text = String::from_utf8_lossy(&buf);
    let ally_set: HashSet<&str> = allies.iter().map(|p| p.puuid.as_str()).collect();
    parse_enemy_puuids(&text, match_id, &ally_set, self_puuid, max)
        .into_iter()
        .map(|puuid| RawPlayer {
            puuid,
            name: None,
            tag: None,
            agent: String::new(),
            team: "Enemy".to_string(),
            party: String::new(),
            level: 0,
            tier_hint: 0,
        })
        .collect()
}

/// Pré-jogo: o payload do glz esconde o time inimigo, mas o endpoint
/// `/loadouts` devolve o `Subject` de todos os jogadores da partida
/// (o cliente usa para pré-carregar as skins). É a fonte primária dos
/// inimigos; o ShooterGame.log fica como reserva.
async fn enemies_from_loadouts(
    ctx: &Ctx,
    match_id: &str,
    known: &[RawPlayer],
    self_puuid: &str,
    max: usize,
) -> Vec<RawPlayer> {
    if max == 0 {
        return vec![];
    }
    let path = format!("/pregame/v1/matches/{}/loadouts", match_id);
    let Ok(v) = ctx.glz(&path).await else { return vec![] };
    let known_set: HashSet<&str> = known.iter().map(|p| p.puuid.as_str()).collect();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for e in v["Loadouts"].as_array().into_iter().flatten() {
        let Some(id) = e["Subject"].as_str() else { continue };
        if id.is_empty()
            || id == self_puuid
            || known_set.contains(id)
            || !seen.insert(id.to_string())
        {
            continue;
        }
        out.push(RawPlayer {
            puuid: id.to_string(),
            name: None,
            tag: None,
            agent: String::new(),
            team: "Enemy".to_string(),
            party: String::new(),
            level: 0,
            tier_hint: 0,
        });
        if out.len() >= max {
            break;
        }
    }
    out
}

async fn build_roster(
    ctx: &Ctx,
    phase: &str,
    priv_me: &Value,
    cache: &mut Caches,
    cfg: &Path,
) -> Option<Value> {
    let base = if phase == "PREGAME" { "/pregame/v1/players/" } else { "/core-game/v1/players/" };
    let pid = match ctx.glz(&format!("{}{}", base, ctx.local.puuid)).await {
        Ok(v) => match v["MatchID"].as_str() {
            Some(id) => id.to_string(),
            None => {
                clog!("[crymora] players-lookup sem MatchID (ainda entrando?)");
                return None;
            }
        },
        Err(e) => {
            clog!("[crymora] players-lookup erro: {e}");
            return None;
        }
    };
    let path = if phase == "PREGAME" {
        format!("/pregame/v1/matches/{pid}")
    } else {
        format!("/core-game/v1/matches/{pid}")
    };
    let m = match ctx.glz(&path).await {
        Ok(m) => m,
        Err(e) => {
            clog!("[crymora] match {pid} erro: {e}");
            return None;
        }
    };

    let mut raw: Vec<RawPlayer> = vec![];
    let mut seen: HashSet<String> = HashSet::new();
    let mut read = |p: &Value, team: &str| {
        let Some(puuid) = player_puuid(p) else { return };
        if !seen.insert(puuid.to_string()) {
            return;
        }
        raw.push(RawPlayer {
            puuid: puuid.to_string(),
            // GameName vazio (perfil oculto no pregame) = sem nome: deixa
            // o namesets local resolver em vez de renderizar em branco
            name: s(p, &["gameName", "GameName"])
                .or_else(|| p["PlayerIdentity"]["GameName"].as_str())
                .or_else(|| p["SeasonalBadgeInfo"]["GameName"].as_str())
                .filter(|n| !n.is_empty())
                .map(String::from),
            tag: s(p, &["tagLine", "TagLine"])
                .or_else(|| p["PlayerIdentity"]["TagLine"].as_str())
                .or_else(|| p["SeasonalBadgeInfo"]["TagLine"].as_str())
                .filter(|t| !t.is_empty())
                .map(String::from),
            agent: s(p, &["CharacterID", "characterId"]).unwrap_or_default().to_string(),
            team: team.to_string(),
            party: s(p, &["PartyID", "partyId", "PartyId"]).unwrap_or_default().to_string(),
            level: p["accountLevel"].as_i64()
                .or_else(|| p["PlayerAccountLevel"].as_i64())
                .or_else(|| p["playerAccountLevel"].as_i64())
                .or_else(|| p["AccountLevel"].as_i64())
                .or_else(|| p["PlayerLevel"].as_i64())
                .or_else(|| p["PlayerIdentity"]["AccountLevel"].as_i64())
                .unwrap_or(0),
            /* pré-jogo: alguns clientes mandam CompetitiveTier=0 para
             * quem está com o perfil oculto, mas o badge sazonal ainda
             * traz o rank. O MMR do pd é o fallback final. */
            tier_hint: p["CompetitiveTier"]
                .as_i64()
                .filter(|t| *t > 0)
                .or_else(|| {
                    p["SeasonalBadgeInfo"]["Rank"]
                        .as_i64()
                        .filter(|t| *t > 0)
                })
                .unwrap_or(0),
        });
    };

    if phase == "PREGAME" {
        /* Pré-jogo: lê os dois times. O payload traz AllyTeam/EnemyTeam
         * além de Teams; Teams já contém ambos, então iteramos tudo e
         * usamos EnemyTeam só como fallback (o `seen` evita duplicar).
         * Aviso: exibir inimigos antes da partida é "scouting" e a Riot
         * classifica como uso não aprovado — o LEIA-ME documenta o risco. */
        let teams = m["Teams"].as_array().cloned().unwrap_or_default();
        for team in &teams {
            let tid = team["TeamID"].as_str().unwrap_or("Blue");
            for p in team["Players"].as_array().into_iter().flatten() {
                read(p, tid);
            }
        }
        if let Some(players) = m["EnemyTeam"]["Players"].as_array() {
            let tid = m["EnemyTeam"]["TeamID"].as_str().unwrap_or("Red");
            for p in players {
                read(p, tid);
            }
        }
    } else {
        // core-game: todos em m.Players com TeamID próprio
        let players = m["Players"].as_array()?;
        let my_team = players
            .iter()
            .find(|p| s(p, &["AccountID", "Subject", "puuid"]) == Some(ctx.local.puuid.as_str()))
            .and_then(|p| p["TeamID"].as_str())
            .unwrap_or("Blue");
        for p in players {
            let tid = p["TeamID"].as_str().unwrap_or(
                if s(p, &["AccountID", "Subject", "puuid"]) == Some(ctx.local.puuid.as_str()) {
                    my_team
                } else {
                    "Enemy"
                },
            );
            read(p, tid);
        }
    }

    drop(read);

    let self_team = raw
        .iter()
        .find(|p| p.puuid == ctx.local.puuid)
        .map(|p| p.team.clone())
        .unwrap_or_else(|| "Blue".to_string());

    /* Pré-jogo: o HTTP só trouxe os aliados. A fonte primária é o
     * endpoint de loadouts, que devolve o Subject dos 10 jogadores;
     * o ShooterGame.log fica como reserva (os inimigos só aparecem
     * nele no fim do agent-select). */
    if phase == "PREGAME" {
        let enemy_size = m["EnemyTeamSize"].as_u64().unwrap_or(5).clamp(1, 10) as usize;
        let mut enemy_count = raw.iter().filter(|p| p.team != self_team).count();
        if enemy_count < enemy_size {
            let found = enemies_from_loadouts(
                ctx,
                &pid,
                &raw,
                &ctx.local.puuid,
                enemy_size - enemy_count,
            )
            .await;
            let added = found.len();
            for p in found {
                if seen.insert(p.puuid.clone()) {
                    raw.push(p);
                }
            }
            enemy_count += added;
            if added > 0 {
                clog!("[crymora] loadouts pregame: {} inimigos", added);
            }
        }
        if enemy_count < enemy_size {
            for p in enemies_from_log(&pid, &raw, &ctx.local.puuid, enemy_size - enemy_count) {
                if seen.insert(p.puuid.clone()) {
                    raw.push(p);
                }
            }
        }
    }

    // core-game não traz nomes — resolve em lote (uma vez, fica em cache)
    let missing: Vec<String> = raw
        .iter()
        .filter(|p| p.name.is_none() && !cache.names.contains_key(&p.puuid))
        .map(|p| p.puuid.clone())
        .collect();
    if !missing.is_empty() && cache.names_backoff <= unix_now() {
        match ctx.names(&missing).await {
            Ok(map) => {
                let resolved = map.len();
                for (k, v) in map {
                    cache.names.insert(k, v);
                }
                // sobra = client local não conhece o puuid e o pd também
                // não devolveu nome — re-tenta no ritmo do backoff
                if resolved < missing.len() {
                    cache.names_backoff = unix_now() + 45;
                }
            }
            Err(e) => {
                clog!("[crymora] names erro: {e} — backoff 45s");
                cache.names_backoff = unix_now() + 45;
            }
        }
    }
    for p in raw.iter_mut() {
        if p.name.is_none() {
            if let Some((n, t)) = cache.names.get(&p.puuid) {
                p.name = Some(n.clone());
                p.tag = Some(t.clone());
            } else {
                // sem nome em nenhuma fonte (client local fora do ar?) —
                // rótulo fixo em vez de nick em branco
                p.name = Some("Oculto".into());
                p.tag = Some(String::new());
            }
        }
    }

    // enriquecimento MMR — sequencial pra não metralhar o pd.
    // No pré-jogo o roster é curto e a partida começa logo: busca todos
    // de uma vez para os dois times (inclusive nick oculto) já aparecerem
    // com rank no primeiro tick.
    let mut players = Vec::with_capacity(raw.len());
    let mut budget = if phase == "PREGAME" { raw.len() } else { 4 };
    let initial_budget = budget;
    let now = unix_now();
    /* monta a lista de quem precisa de consulta e dispara tudo de uma vez;
     * cache-fresco/backoff não consome cota */
    let mut fetch: Vec<String> = Vec::new();
    for p in &raw {
        if budget > 0 && mmr_needs_fetch(cache, &p.puuid, now) {
            budget -= 1;
            fetch.push(p.puuid.clone());
        }
    }
    let fetched_any = !fetch.is_empty();
    let mut results: HashMap<String, Result<MmrInfo, String>> =
        fetch.iter().cloned().zip(mmr_batch(ctx, &fetch).await).collect();
    for p in &raw {
        let m = match results.remove(&p.puuid) {
            Some(res) => mmr_store(cache, &p.puuid, res, now),
            None => cache.mmr.get(&p.puuid).map(mmr_decode),
        };
        players.push(build_player(p, m.as_ref(), p.puuid == ctx.local.puuid));
    }
    if fetched_any {
        save_json_map(&cfg.join("mmr-cache.json"), &cache.mmr);
    }
    let with_rank = players
        .iter()
        .filter(|p| p["competitiveTier"].as_i64().unwrap_or(0) > 0)
        .count();
    clog!(
        "[crymora] roster ranks: {}/{} (mmr novos neste tick: {})",
        with_rank,
        players.len(),
        initial_budget - budget
    );
    let (allies, mut enemies): (Vec<Value>, Vec<Value>) = players
        .into_iter()
        .partition(|p| p["teamId"].as_str() == Some(self_team.as_str()));

    // Fallback garantido: se enemies vazio, usa-se o time contrario
    // do array Teams para garantir 5vs5 na UI. Isso ocorre quando o
    // payload do pré-jogo n veio com EnemyTeam explícito.
    if enemies.is_empty() {
        if let Some(teams) = m["Teams"].as_array() {
            for team in teams {
                let tid = team["TeamID"].as_str();
                if let Some(t) = tid {
                    if t != &self_team {
                        if let Some(plist) = team["Players"].as_array() {
                            for p in plist {
                                // buscar no raw o player correspondente
                                if let Some(rp) = raw.iter().find(|r| r.puuid == p["puuid"].as_str().or_else(|| p["Subject"].as_str()).unwrap_or("")) {
                                    enemies.push(build_player(rp, None, false));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // modo/mapa: a presença própria já carrega (fallback no MatchInfo)
    let _me = remote::priv_info(priv_me);
    let me = remote::priv_info(priv_me);
    let mode = if me.queue_id.is_empty() {
        m["MatchInfo"]["ModeID"].as_str().unwrap_or("").to_string()
    } else {
        me.queue_id.clone()
    };
    let map_key = if me.map_key.is_empty() {
        m["MatchInfo"]["MapID"].as_str().unwrap_or("").to_string()
    } else {
        me.map_key.clone()
    };

    Some(json!({
        "allies": allies,
        "enemyTeam": enemies,
        "mode": mode,
        "mapKey": map_key,
    }))
}

/// Próprio grupo (partyId + membros) via glz. O payload da partida não
/// traz PartyID (o Riot removeu), então esta é a única fonte ao vivo
/// para marcar quem está no seu grupo — inclusive quem não é amigo.
async fn self_party(ctx: &Ctx, cache: &mut Caches) -> Option<(String, Vec<String>)> {
    let now = unix_now();
    if let Some((id, members, ts)) = &cache.self_party {
        if now - *ts < PARTY_TTL_SECS {
            return Some((id.clone(), members.clone()));
        }
    }
    if cache.party_backoff > now {
        return cache
            .self_party
            .as_ref()
            .map(|(id, members, _)| (id.clone(), members.clone()));
    }
    let player = match ctx
        .glz(&format!("/parties/v1/players/{}", ctx.local.puuid))
        .await
    {
        Ok(v) => v,
        Err(e) => {
            clog!("[crymora] party player erro: {e} — retry em {PARTY_RETRY_SECS}s");
            cache.party_backoff = now + PARTY_RETRY_SECS;
            return cache
                .self_party
                .as_ref()
                .map(|(id, members, _)| (id.clone(), members.clone()));
        }
    };
    let Some(id) = player["CurrentPartyID"].as_str().filter(|s| !s.is_empty()) else {
        /* saiu do grupo (ou solo): não deixa o grupo antigo marcado */
        cache.self_party = None;
        cache.party_backoff = now + PARTY_TTL_SECS;
        return None;
    };
    let party = match ctx.glz(&format!("/parties/v1/parties/{id}")).await {
        Ok(v) => v,
        Err(e) => {
            clog!("[crymora] party erro: {e} — retry em {PARTY_RETRY_SECS}s");
            cache.party_backoff = now + PARTY_RETRY_SECS;
            return None;
        }
    };
    let members: Vec<String> = party["Members"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter_map(|m| m["Subject"].as_str())
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    if members.is_empty() {
        cache.party_backoff = now + PARTY_RETRY_SECS;
        return None;
    }
    cache.party_backoff = 0;
    cache.self_party = Some((id.to_string(), members.clone(), now));
    Some((id.to_string(), members))
}

/// Injeta `partyId`/`partySize` no roster. Preferência:
///   1. próprio grupo (API de party) — cobre membros que não são amigos;
///   2. presença do amigo (partyId do packedData) — cobre amigos no lobby.
/// Ninguém além dessas fontes é marcado: o payload da partida não tem
/// PartyID, então inimigos desconhecidos ficam sem grupo (não inventamos).
fn inject_parties(
    roster: &mut Value,
    own: Option<&(String, Vec<String>)>,
    friends: &HashMap<String, (String, i64)>,
) {
    let Some(obj) = roster.as_object_mut() else { return };
    for key in ["allies", "enemyTeam"] {
        let Some(list) = obj.get_mut(key).and_then(|v| v.as_array_mut()) else { continue };
        for p in list {
            let Some(puuid) = p["puuid"].as_str().map(String::from) else { continue };
            if let Some((pid, members)) = own {
                if members.iter().any(|m| m == &puuid) {
                    p["partyId"] = json!(pid);
                    p["partySize"] = json!(members.len());
                    continue;
                }
            }
            if p["partyId"].as_str().map(|s| s.is_empty()).unwrap_or(true) {
                if let Some((pid, size)) = friends.get(&puuid) {
                    p["partyId"] = json!(pid);
                    p["partySize"] = json!(size);
                }
            }
        }
    }
}

/// Todos os jogadores do roster têm elo confirmado? Unranked carregado
/// (tier 0 com `rankResolved`) conta como confirmado.
fn ranks_ready(roster: &Value) -> bool {
    let mut total = 0;
    for key in ["allies", "enemyTeam"] {
        if let Some(list) = roster[key].as_array() {
            total += list.len();
            if list
                .iter()
                .any(|p| !p["rankResolved"].as_bool().unwrap_or(false))
            {
                return false;
            }
        }
    }
    total > 0
}

/// Decide se o pré-jogo fica escondido até os elos chegarem. Depois da
/// primeira revelação não esconde mais (evita piscar quando alguém novo
/// entra); o timeout libera a tela se o pd recusar alguma consulta.
fn pregame_hold(
    phase: &str,
    roster: Option<&Value>,
    cache: &mut Caches,
    now: i64,
) -> bool {
    if phase != "PREGAME" {
        cache.pregame_since = None;
        cache.pregame_shown = false;
        return false;
    }
    let since = *cache.pregame_since.get_or_insert(now);
    if cache.pregame_shown {
        return false;
    }
    if roster.map(ranks_ready).unwrap_or(false) || now - since >= RANK_WAIT_SECS {
        cache.pregame_shown = true;
        return false;
    }
    true
}

/* ------------------------------------------------------------------ */
/* friends (presences)                                                 */
/* ------------------------------------------------------------------ */

async fn build_friends(
    ctx: Option<&Ctx>,
    pres_arr: &[Value],
    cache: &mut Caches,
    cfg: &Path,
) -> Value {
    let Ok(f) = remote::local("/chat/v4/friends").await else {
        clog!("[crymora] friends erro: /chat/v4/friends falhou");
        return json!({ "friends": [] });
    };

    let friends = f["friends"].as_array().cloned().unwrap_or_default();

    // 2026: cada pessoa tem VÁRIAS presenças (uma por produto — valorant,
    // riot_client, lol…). A do jogo é a que tem sessão/tier; as outras só
    // dizem chat/offline. Mantém a do valorant se existir.
    let mut pres_by: HashMap<&str, &Value> = HashMap::new();
    for p in pres_arr {
        let Some(id) = p["puuid"].as_str() else { continue };
        let keep = match pres_by.get(id) {
            Some(cur) => {
                p["product"].as_str() == Some("valorant")
                    && cur["product"].as_str() != Some("valorant")
            }
            None => true,
        };
        if keep {
            pres_by.insert(id, p);
        }
    }
    // partySize: quem compartilha partyId (presenças incluem o próprio)
    let mut party_count: HashMap<String, i64> = HashMap::new();
    for p in pres_arr {
        let pid = remote::presence_priv_info(p).party_id;
        if !pid.is_empty() {
            *party_count.entry(pid).or_insert(0) += 1;
        }
    }

    let mut out = Vec::new();
    let mut dirty = false;
    /* elo dos amigos: presença e friends-cache podem estar de um ato
     * antigo; o pd é a fonte do ato vigente. Até 4 consultas novas por
     * tick, agora em paralelo — o resto completa nos ticks seguintes. */
    let now = unix_now();
    let mut mmr_budget = 4;
    let mut mmr_fetch: Vec<String> = Vec::new();
    if ctx.is_some() {
        for fr in &friends {
            let Some(puuid) = fr["puuid"].as_str() else { continue };
            if mmr_budget > 0 && mmr_needs_fetch(cache, puuid, now) {
                mmr_budget -= 1;
                mmr_fetch.push(puuid.to_string());
            }
        }
    }
    let mmr_fetched = !mmr_fetch.is_empty();
    let mut mmr_results: HashMap<String, Result<MmrInfo, String>> = match ctx {
        Some(ctx) => mmr_fetch
            .iter()
            .cloned()
            .zip(mmr_batch(ctx, &mmr_fetch).await)
            .collect(),
        None => HashMap::new(),
    };
    for fr in friends {
        let Some(puuid) = fr["puuid"].as_str() else { continue };
        let pres = pres_by.get(puuid);
        let pi = match pres {
            Some(p) => remote::presence_priv_info(p),
            None => remote::Priv::default(),
        };
        // offline: presença não traz elo/level/card → último conhecido
        let c = cache.friends.get(puuid).cloned().unwrap_or(Value::Null);
        let live_tier = pi.tier > 0;
        let mut tier = if live_tier { pi.tier } else { c["tier"].as_i64().unwrap_or(0) };
        let level = if pi.level > 0 { pi.level } else { c["level"].as_i64().unwrap_or(0) };
        let card = if !pi.card.is_empty() {
            pi.card.clone()
        } else {
            c["card"].as_str().unwrap_or("").to_string()
        };

        // MMR do pd: presença ao vivo manda no tier; sem presença (offline
        // ou só LoL) o cache local pode ser de ato antigo, então o pd manda.
        let mmr = match mmr_results.remove(puuid) {
            Some(res) => mmr_store(cache, puuid, res, now),
            None => cache
                .mmr
                .get(puuid)
                .filter(|e| e["v"].as_i64() == Some(MMR_CACHE_VERSION))
                .map(mmr_decode),
        };
        if let Some(m) = mmr.as_ref().filter(|m| m.tier > 0) {
            if !live_tier {
                tier = m.tier;
            }
        }
        let mmr_rank = mmr.as_ref().filter(|m| m.tier > 0 && m.tier == tier);

        // persiste o durável: tier do pd completa o cache de presença
        let entry = json!({ "tier": tier, "level": level, "card": card.clone() });
        if cache.friends.get(puuid) != Some(&entry) {
            cache.friends.insert(puuid.to_string(), entry);
            dirty = true;
        }
        let party_id = pi.party_id.clone();
        // tamanho real vem no packedData; fallback = contar presenças
        let party_size = pres
            .and_then(|p| remote::presence_party_size(p))
            .or_else(|| party_count.get(&party_id).copied())
            .unwrap_or(1);
        out.push(json!({
            "puuid": puuid,
            "gameName": s(&fr, &["game_name", "name"])
                .or_else(|| pres.and_then(|p| p["game_name"].as_str()))
                .unwrap_or_default(),
            "tagLine": s(&fr, &["tag_line", "tagLine"])
                .or_else(|| pres.and_then(|p| p["game_tag"].as_str()))
                .unwrap_or_default(),
            // 2026: `state` substituiu `availability`. "mobile" = só o
            // celular/companion (ex.: LoL) — o client do jogo mostra
            // offline; era o "online fantasma" da lista
            "availability": if let Some(p) = pres {
                let st = remote::presence_state(*p);
                if st.is_empty() || st == "mobile" { "offline".into() } else { st }
            } else { "offline".into() },
            "product": pres.and_then(|p| p["product"].as_str()).unwrap_or(""),
            "sessionState": pi.session_state,
            "queueId": pi.queue_id,
            "mapKey": pi.map_key,
            "scoreAlly": pi.score_ally,
            "scoreEnemy": pi.score_enemy,
            "competitiveTier": tier,
            "rankedRating": mmr_rank.map(|m| m.rr),
            "rankResolved": mmr_rank.is_some(),
            /* perfil completo (mesmo contrato do roster) — o modal de
             * estatísticas do amigo usa estes campos sem outra consulta */
            "peakTier": mmr.as_ref().map(|m| m.peak),
            "peakResolved": mmr.is_some(),
            "actPeakTier": mmr.as_ref().and_then(|m| m.act_peak),
            "actWins": mmr.as_ref().map(|m| m.act_wins).unwrap_or(0),
            "actGames": mmr.as_ref().map(|m| m.act_games).unwrap_or(0),
            "prevAct": mmr.as_ref().and_then(|m| m.prev).map(|(t, w, g)| {
                json!({ "tier": t, "wins": w, "games": g })
            }).unwrap_or(Value::Null),
            "careerGames": mmr.as_ref().map(|m| m.career_games).unwrap_or(0),
            "leaderboardRank": mmr.as_ref().and_then(|m| m.leaderboard),
            "form": [],
            "accountLevel": level,
            "partyId": party_id.clone(),
            "partySize": party_size,
            "lastOnlineUnix": fr["last_online_ts"].as_i64()
                .or_else(|| pres.and_then(|p| p["time"].as_i64()))
                .map(|ms| ms / 1000),
            "playerCardId": card,
        }));
    }
    if mmr_fetched {
        save_json_map(&cfg.join("mmr-cache.json"), &cache.mmr);
    }
    if dirty {
        save_json_map(&cfg.join("friends-cache.json"), &cache.friends);
    }
    json!({ "friends": out })
}

/* ------------------------------------------------------------------ */
/* rótulos p/ Discord (mesmo vocabulário pt da UI)                      */
/* ------------------------------------------------------------------ */

const MAP_LABELS: &[(&str, &str)] = &[
    ("ascent", "Ascent"), ("duality", "Bind"), ("triad", "Haven"),
    ("bonsai", "Split"), ("port", "Icebox"), ("foxtrot", "Breeze"),
    ("canyon", "Fracture"), ("pitt", "Pearl"), ("jam", "Lotus"),
    ("juliett", "Sunset"), ("infinity", "Abyss"), ("rook", "Corrode"),
    ("plummet", "Summit"), ("range", "The Range"),
];
const MODE_LABELS: &[(&str, &str)] = &[
    ("competitive", "Competitivo"), ("unrated", "Sem Ranque"),
    ("swiftplay", "Partida Rápida"), ("spikerush", "Spike Rush"),
    ("deathmatch", "Mata-mata"), ("hurm", "Mata-mata em equipes"),
    ("ggteam", "Escalonamento"), ("onefa", "Replicação"),
    ("newmap", "Padrão"), ("fortcollins", "Retomada"),
    ("retake", "Retomada"), ("snowball", "Guerra de Bolas de Neve"),
    ("custom", "Personalizado"),
];

fn display(key: &str, table: &[(&str, &str)]) -> String {
    let seg = remote::last_seg(key);
    if seg.is_empty() {
        return String::new();
    }
    let lower = seg.to_lowercase();
    match table.iter().find(|(k, _)| *k == lower) {
        Some((_, n)) => n.to_string(),
        None => {
            let mut c = seg.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    }
}

/* ------------------------------------------------------------------ */
/* tick                                                                */
/* ------------------------------------------------------------------ */

async fn tick(
    region: &mut Option<Region>,
    local: &mut Option<Local>,
    cache: &mut Caches,
    cfg: &Path,
) -> (Snapshot, RpInfo) {
    let started = std::time::Instant::now();
    if lockfile::read().is_none() {
        clog!("[crymora] lockfile ausente");
        *local = None;
        return offline(false);
    }
    /* Revalida a sessão a cada tick: o Riot Client troca de conta sem
     * reiniciar o processo e renova o access token. Sem isso, o puuid
     * antigo continuava mandando na detecção de fase/região e a lista de
     * amigos podia ficar presa na conta anterior até reiniciar o app. */
    match Local::load().await {
        Ok(fresh) => {
            if let Some(old) = local.as_ref() {
                if old.puuid != fresh.puuid {
                    clog!(
                        "[crymora] sessão trocada: {}… → {}…",
                        old.puuid.chars().take(8).collect::<String>(),
                        fresh.puuid.chars().take(8).collect::<String>()
                    );
                    *region = None;
                }
            }
            *local = Some(fresh);
        }
        Err(e) => {
            if local.is_none() {
                clog!("[crymora] entitlements falhou: {e}");
                return offline(true);
            }
            /* conta em troca/logout: mantém a sessão anterior por alguns
             * ticks; o próximo /entitlements válido corrige tudo */
        }
    }
    let Some(loc) = local else { return offline(true) };

    /* troca de conta pelo puuid — inclusive quando a sessão antiga caiu
     * para `None` durante o switch (o caminho antigo comparava `local`
     * com o token novo e não rodava). Sem isso o nome próprio e o cache
     * de amigos ficavam presos na conta anterior. */
    if cache.last_self_puuid.as_deref() != Some(loc.puuid.as_str()) {
        let switched = cache.last_self_puuid.is_some();
        cache.last_self_puuid = Some(loc.puuid.clone());
        cache.self_name_tried = false;
        if switched {
            cache.names_backoff = 0;
            cache.friends.clear();
            cache.pregame_since = None;
            cache.pregame_shown = false;
            save_json_map(&cfg.join("friends-cache.json"), &cache.friends);
            clog!(
                "[crymora] conta ativa: {}…",
                loc.puuid.chars().take(8).collect::<String>()
            );
        }
    }

    let presences = match remote::local("/chat/v4/presences").await {
        Ok(v) => v,
        Err(e) => {
            clog!("[crymora] presences erro: {e}");
            *local = None; // client caiu ou lockfile velho → reconecta no próximo tick
            return offline(true);
        }
    };
    let pres_arr = presences["presences"].as_array().cloned().unwrap_or_default();
    clog!("[crymora] presences: {} · phase-source ok", pres_arr.len());

    /* partyId por puuid nas presenças (amigos). O payload da partida não
     * traz PartyID; para amigos no lobby/partida a presença é a fonte. */
    let mut friends_party: HashMap<String, (String, i64)> = HashMap::new();
    for p in &pres_arr {
        let Some(puuid) = p["puuid"].as_str() else { continue };
        let info = remote::presence_priv_info(p);
        if info.party_id.is_empty() {
            continue;
        }
        let size = remote::presence_party_size(p).unwrap_or(1);
        friends_party.insert(puuid.to_string(), (info.party_id, size));
    }

    if region.is_none() {
        *region = region::detect();
        if region.is_none() {
            clog!("[crymora] ShooterGame.log sem host glz (região não detectada)");
        } else {
            clog!("[crymora] região: {}/{}", region.as_ref().unwrap().region, region.as_ref().unwrap().shard);
        }
    }

    // própria presença: cada puuid tem VÁRIAS entradas (valorant,
    // riot_client, lol…) — a do valorant é a que carrega `private`;
    // a do riot_client vem vazia e, se estiver antes na lista, quebrava
    // a detecção de fase
    let self_pres = pres_arr
        .iter()
        .filter(|p| p["puuid"].as_str() == Some(loc.puuid.as_str()))
        .max_by_key(|p| if p["product"].as_str() == Some("valorant") { 1 } else { 0 });
    let priv_me = self_pres
        .map(remote::presence_private)
        .unwrap_or(Value::Null);
    if priv_me.is_null() {
        clog!("[crymora] própria presença sem `private` decodificável");
    }
    /* o chat sempre carrega o Riot ID do próprio jogador — fallback
     * imediato quando o nameset local ainda não conhece o puuid novo */
    let self_pres_name = self_pres.and_then(|p| {
        let n = p["game_name"].as_str().unwrap_or("").trim();
        let t = p["game_tag"].as_str().unwrap_or("").trim();
        (!n.is_empty()).then(|| (n.to_string(), t.to_string()))
    });
    let me = remote::priv_info(&priv_me);
    let phase = match me.session_state.to_ascii_uppercase().as_str() {
        "PREGAME" => "PREGAME",
        "INGAME" => "INGAME",
        _ => "MENUS",
    };

    // contexto remoto (glz/pd) — também alimenta o elo dos amigos
    let ctx = match region.clone() {
        Some(reg) => Some(Ctx::new(reg, loc.clone(), remote::client_version().await)),
        None => None,
    };

    // Riot ID próprio — uma tentativa por puuid (troca de conta rearma)
    if !cache.self_name_tried && !cache.names.contains_key(&loc.puuid) {
        cache.self_name_tried = true;
        if let Some(ctx) = ctx.as_ref() {
            if let Ok(map) = ctx.names(&[loc.puuid.clone()]).await {
                for (k, v) in map {
                    cache.names.insert(k, v);
                }
            }
        }
    }
    if !cache.names.contains_key(&loc.puuid) {
        if let Some(id) = self_pres_name {
            cache.names.insert(loc.puuid.clone(), id);
        }
    }
    let self_name = cache.names.get(&loc.puuid).map(|(n, _)| n.clone()).unwrap_or_default();

    // roster: só com região (glz/pd) — igual ao original ("start VALORANT")
    let mut roster = if phase == "MENUS" {
        None
    } else if let Some(ctx) = ctx.as_ref() {
        build_roster(ctx, phase, &priv_me, cache, cfg).await
    } else {
        None
    };
    /* grupos ao vivo: o payload da partida não tem PartyID. Marca o
     * próprio grupo (API de party) e amigos que estejam na partida
     * (presença). Inimigos desconhecidos ficam sem marca. */
    if let (Some(ctx), Some(r)) = (ctx.as_ref(), roster.as_mut()) {
        let party = self_party(ctx, cache).await;
        inject_parties(r, party.as_ref(), &friends_party);
    }

    let hold_pregame = pregame_hold(phase, roster.as_ref(), cache, unix_now());

    let friends = build_friends(ctx.as_ref(), &pres_arr, cache, cfg).await;
    clog!(
        "[crymora] tick ok · phase={phase} · friends={} · roster={}{} · {}ms",
        friends["friends"].as_array().map(|a| a.len()).unwrap_or(0),
        roster.is_some(),
        if hold_pregame { " · aguardando elos" } else { "" },
        started.elapsed().as_millis()
    );

    let status = json!({
        "clientRunning": true,
        "valorantRunning": shooter_log_fresh() || phase != "MENUS",
        "loggedIn": true,
        "region": region.as_ref().map(|r| r.region.clone()),
        "shard": region.as_ref().map(|r| r.shard.clone()),
        "puuid": loc.puuid,
        "selfName": self_name,
    });

    // presence: "mapa · modo" + estado (placar/pré-jogo) — formato pedido
    let (details, rp_state) = match phase {
        "PREGAME" => {
            let map = display(&me.map_key, MAP_LABELS);
            let mode = display(&me.queue_id, MODE_LABELS);
            let details = [map, mode].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            (details, "Pré-jogo".to_string())
        }
        "INGAME" => {
            let map = display(&me.map_key, MAP_LABELS);
            let mode = display(&me.queue_id, MODE_LABELS);
            let details = [map, mode].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
            let state = match (me.score_ally, me.score_enemy) {
                (Some(a), Some(b)) => format!("{a} – {b}"),
                _ => "Em partida".to_string(),
            };
            (details, state)
        }
        _ => ("No menu".to_string(), "Aguardando partida".to_string()),
    };

    let (pub_phase, pub_roster) = if hold_pregame {
        ("MENUS", None)
    } else {
        (phase, roster)
    };

    (
        Snapshot {
            match_state: json!({ "phase": pub_phase }),
            roster: pub_roster,
            friends,
            riot_status: status,
        },
        RpInfo { phase: phase.to_string(), details, state: rp_state },
    )
}

fn offline(client_running: bool) -> (Snapshot, RpInfo) {
    (
        Snapshot {
            match_state: json!({ "phase": "MENUS" }),
            roster: None,
            friends: json!({ "friends": [] }),
            riot_status: json!({
                "clientRunning": client_running,
                "valorantRunning": false,
                "loggedIn": false,
                "region": null, "shard": null,
                "puuid": "", "selfName": "",
            }),
        },
        RpInfo { phase: "MENUS".into(), details: String::new(), state: String::new() },
    )
}

/* ------------------------------------------------------------------ */
/* loop principal                                                      */
/* ------------------------------------------------------------------ */

/// Estado mutável do poll (sessão + caches). Fica num Mutex para que o
/// comando `refresh_now` rode um tick na hora, sem esperar o intervalo —
/// é o que faz o Alt+Tab voltar com tudo atualizado.
pub struct Poller {
    region: Option<Region>,
    local: Option<Local>,
    cache: Caches,
    cfg: PathBuf,
}

impl Poller {
    pub fn new(app: &AppHandle) -> Self {
        let cfg = config_dir(app);
        Self {
            region: None,
            local: None,
            cache: Caches {
                mmr: load_json_map(&cfg.join("mmr-cache.json")),
                names: HashMap::new(),
                friends: load_json_map(&cfg.join("friends-cache.json")),
                mmr_backoff: HashMap::new(),
                names_backoff: 0,
                self_name_tried: false,
                last_self_puuid: None,
                pregame_since: None,
                pregame_shown: false,
                self_party: None,
                party_backoff: 0,
            },
            cfg,
        }
    }

    pub async fn tick(&mut self) -> (Snapshot, RpInfo) {
        tick(&mut self.region, &mut self.local, &mut self.cache, &self.cfg).await
    }
}

pub async fn poll_loop(
    app: AppHandle,
    shared: crate::Shared,
    poller: std::sync::Arc<tokio::sync::Mutex<Poller>>,
) {
    let mut rp = DiscordRp::new();

    clog!("[crymora] poll_loop iniciado");
    loop {
        let (snap, info) = poller.lock().await.tick().await;
        *shared.lock().await = snap;

        let enabled = crate::settings::get(&app)
            .ok()
            .and_then(|v| v["discordRpEnabled"].as_bool())
            .unwrap_or(false);
        rp.update(enabled, &info.phase, &info.details, &info.state);

        let wait = if info.phase == "MENUS" { TICK_IDLE_MS } else { TICK_ACTIVE_MS };
        tokio::time::sleep(Duration::from_millis(wait)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_cache() -> Caches {
        Caches {
            mmr: Map::new(),
            names: HashMap::new(),
            friends: Map::new(),
            mmr_backoff: HashMap::new(),
            names_backoff: 0,
            self_name_tried: false,
            last_self_puuid: None,
            pregame_since: None,
            pregame_shown: false,
            self_party: None,
            party_backoff: 0,
        }
    }

    #[test]
    fn ranks_ready_requires_every_player_resolved() {
        let ok = |tier: i64| json!({
            "rankResolved": true,
            "rank": { "tier": tier, "rr": 0 },
        });

        /* todos carregados, incluindo um unranked confirmado (tier 0) */
        let ready = json!({
            "allies": [ok(18), ok(0)],
            "enemyTeam": [ok(21), ok(9)],
        });
        assert!(ranks_ready(&ready));

        /* um jogador ainda sem confirmação segura a partida */
        let pending = json!({
            "allies": [ok(18)],
            "enemyTeam": [json!({ "rankResolved": false, "competitiveTier": 20 })],
        });
        assert!(!ranks_ready(&pending));

        /* roster vazio não é "pronto" */
        let empty = json!({ "allies": [], "enemyTeam": [] });
        assert!(!ranks_ready(&empty));
    }

    #[test]
    fn injects_party_ids_from_own_party_and_friends() {
        let mut roster = json!({
            "allies": [
                { "puuid": "a", "partyId": "" },
                { "puuid": "b", "partyId": "" },
                { "puuid": "c", "partyId": "" },
            ],
            "enemyTeam": [
                { "puuid": "d", "partyId": "" },
                { "puuid": "e", "partyId": "" },
            ],
        });
        let own = ("own-party".to_string(), vec!["a".to_string(), "b".to_string()]);
        let mut friends = HashMap::new();
        friends.insert("d".to_string(), ("friend-party".to_string(), 2));

        inject_parties(&mut roster, Some(&own), &friends);

        /* membros do próprio grupo — inclusive quem não é amigo */
        assert_eq!(roster["allies"][0]["partyId"], "own-party");
        assert_eq!(roster["allies"][1]["partyId"], "own-party");
        assert_eq!(roster["allies"][0]["partySize"], 2);
        /* fora do grupo continua sem marca */
        assert_eq!(roster["allies"][2]["partyId"], "");
        /* amigo no time inimigo usa o partyId da presença */
        assert_eq!(roster["enemyTeam"][0]["partyId"], "friend-party");
        assert_eq!(roster["enemyTeam"][0]["partySize"], 2);
        assert_eq!(roster["enemyTeam"][1]["partyId"], "");
    }

    #[test]
    fn own_party_wins_over_presence_party() {
        let mut roster = json!({
            "allies": [{ "puuid": "a", "partyId": "stale" }],
            "enemyTeam": [],
        });
        let own = ("own-party".to_string(), vec!["a".to_string()]);
        inject_parties(&mut roster, Some(&own), &HashMap::new());
        assert_eq!(roster["allies"][0]["partyId"], "own-party");
    }

    #[test]
    fn pregame_waits_then_reveals_once() {
        let mut cache = empty_cache();
        let pending = json!({
            "allies": [{ "rankResolved": false }],
            "enemyTeam": [],
        });
        let ready = json!({
            "allies": [{ "rankResolved": true, "rank": { "tier": 0 } }],
            "enemyTeam": [],
        });

        assert!(pregame_hold("PREGAME", Some(&pending), &mut cache, 100));
        assert!(pregame_hold("PREGAME", Some(&pending), &mut cache, 101));
        /* elos chegaram (unranked confirmado conta) → revela */
        assert!(!pregame_hold("PREGAME", Some(&ready), &mut cache, 102));
        /* depois de revelado, jogador novo sem elo não esconde de novo */
        assert!(!pregame_hold("PREGAME", Some(&pending), &mut cache, 103));
        /* saiu do pré-jogo → estado limpo para a próxima */
        assert!(!pregame_hold("MENUS", None, &mut cache, 104));
        assert!(cache.pregame_since.is_none());
        assert!(!cache.pregame_shown);
    }

    #[test]
    fn pregame_timeout_releases_without_ranks() {
        let mut cache = empty_cache();
        let pending = json!({
            "allies": [{ "rankResolved": false }],
            "enemyTeam": [],
        });

        assert!(pregame_hold("PREGAME", Some(&pending), &mut cache, 100));
        assert!(pregame_hold("PREGAME", Some(&pending), &mut cache, 104));
        /* 5s sem todos os elos → mostra com o que veio */
        assert!(!pregame_hold("PREGAME", Some(&pending), &mut cache, 105));
    }

    #[test]
    fn enemy_puuid_falls_back_to_player_identity() {
        /* payload real do pré-jogo: Subject de cima nulo no inimigo */
        let enemy = json!({
            "Subject": null,
            "PlayerIdentity": { "Subject": "11111111-2222-3333-4444-555555555555" },
            "SeasonalBadgeInfo": { "Rank": 18 },
        });
        assert_eq!(
            player_puuid(&enemy),
            Some("11111111-2222-3333-4444-555555555555")
        );

        /* aliado: Subject normal continua funcionando */
        let ally = json!({
            "Subject": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "PlayerIdentity": { "Subject": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee" },
        });
        assert_eq!(
            player_puuid(&ally),
            Some("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
        );

        /* Subject vazio também cai no PlayerIdentity */
        let empty = json!({
            "Subject": "",
            "PlayerIdentity": { "Subject": "99999999-8888-7777-6666-555555555555" },
        });
        assert_eq!(
            player_puuid(&empty),
            Some("99999999-8888-7777-6666-555555555555")
        );
    }

    #[test]
    fn parses_enemy_puuids_from_game_log() {
        let log = concat!(
            "[..] Pregame_GetMatch ... 02b0d003 ... Response Code: [200]\n",
            "[..] Subject had empty display name. aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa\n",
            "[..] Subject had empty display name. bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb\n",
            "[..] Subject had empty display name. bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb\n",
            "[..] Subject had empty display name. cccccccc-cccc-cccc-cccc-cccccccccccc\n",
            "[..] Subject had empty display name. dddddddd-dddd-dddd-dddd-dddddddddddd\n",
            "[..] Subject had empty display name. eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee\n",
            "[..] Subject had empty display name. ffffffff-ffff-ffff-ffff-ffffffffffff\n",
            "[..] Subject had empty display name. 99999999-9999-9999-9999-999999999999\n",
        );
        let allies: HashSet<&str> =
            ["aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"].into_iter().collect();
        let out = parse_enemy_puuids(
            log,
            "02b0d003",
            &allies,
            "99999999-9999-9999-9999-999999999999",
            5,
        );
        assert_eq!(out.len(), 5);
        assert_eq!(out[0], "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb");
        assert!(!out.contains(&"aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa".to_string()));
        assert!(!out.contains(&"99999999-9999-9999-9999-999999999999".to_string()));
    }

    #[test]
    fn enemy_log_anchor_stays_on_first_query_of_the_match() {
        /* O app consulta o Pregame_GetMatch a cada tick; se a âncora
         * fosse a última consulta, os inimigos (que aparecem no fim do
         * agent-select) ficariam para trás. A âncora é a primeira. */
        let log = concat!(
            "[..] Pregame_GetMatch ... 02b0d003 ... Response Code: [200]\n",
            "[..] Subject had empty display name. bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb\n",
            "[..] Pregame_GetMatch ... 02b0d003 ... Response Code: [200]\n",
            "[..] Subject had empty display name. cccccccc-cccc-cccc-cccc-cccccccccccc\n",
            "[..] Pregame_GetMatch ... 11111111-1111-1111-1111-111111111111 ... Response Code: [200]\n",
            "[..] Subject had empty display name. dddddddd-dddd-dddd-dddd-dddddddddddd\n",
        );
        let allies: HashSet<&str> = HashSet::new();
        let out = parse_enemy_puuids(log, "02b0d003", &allies, "self", 10);
        assert_eq!(
            out,
            vec![
                "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb".to_string(),
                "cccccccc-cccc-cccc-cccc-cccccccccccc".to_string(),
            ]
        );
    }
}
