/* Região/shard — evidência do binário original:
 *   src\riot\region.rs parseia %LOCALAPPDATA%\VALORANT\Saved\Logs\
 *   ShooterGame.log com o regex glz-([a-z]+)-\d+\.([a-z]+)\.a\.pvp\.net
 *   ("valid host regex") → grupo 1 = região, grupo 2 = shard.
 *   erro: "Riot region not detected (start VALORANT)" */

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Region {
    pub region: String,
    pub shard: String,
}

pub fn detect() -> Option<Region> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let path = PathBuf::from(local)
        .join("VALORANT")
        .join("Saved")
        .join("Logs")
        .join("ShooterGame.log");
    let log = std::fs::read_to_string(path).ok()?;
    parse_log(&log)
}

/// Versão exata do jogo em execução — a mesma que o jogo manda em
/// X-Riot-ClientVersion. Linha no log:
///   LogShooter: Display: CI server version: release-13.08-shipping-22-5760300
/// (o valorant-api atrasa uns builds; essa aqui é sempre a atual).
/// Fica perto do topo (inicialização) — ler 64KB basta, e o log pode
/// ter dezenas de MB em sessão longa.
pub fn client_version_from_log() -> Option<String> {
    use std::io::Read as _;
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let path = PathBuf::from(local)
        .join("VALORANT")
        .join("Saved")
        .join("Logs")
        .join("ShooterGame.log");
    let mut file = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; 64 * 1024];
    let n = file.read(&mut buf).ok()?;
    let head = String::from_utf8_lossy(&buf[..n]);
    head.lines()
        .find_map(|l| l.split_once("CI server version: "))
        .map(|(_, v)| v.trim().to_string())
        .filter(|v| v.starts_with("release-"))
}

fn parse_log(log: &str) -> Option<Region> {
    // varre de trás pra frente: as entradas mais recentes ficam no fim
    for line in log.lines().rev() {
        // glz-<region>-<n>.<shard>.a.pvp.net  (ex.: glz-na-1.na.a.pvp.net)
        if let Some(pos) = line.find("glz-") {
            let host: String = line[pos + 4..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
                .collect();
            let Some(stripped) = host.strip_suffix(".a.pvp.net") else { continue };
            let mut it = stripped.splitn(2, '.');
            let reg_num = it.next().unwrap_or(""); // "na-1"
            let shard = it.next().unwrap_or("");   // "na"
            let region = reg_num.split('-').next().unwrap_or("");
            if !region.is_empty() && !shard.is_empty() {
                return Some(Region { region: region.to_string(), shard: shard.to_string() });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_glz_host() {
        let log = "x\nLogLyra: https://glz-na-1.na.a.pvp.net/pregame/v1/players/abc\n";
        let r = parse_log(log).unwrap();
        assert_eq!(r.region, "na");
        assert_eq!(r.shard, "na");
    }
}
