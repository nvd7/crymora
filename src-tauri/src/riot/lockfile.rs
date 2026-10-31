/* Lockfile do Riot Client — evidência do binário original:
 *   %LOCALAPPDATA%\Riot Games\Riot Client\Config\lockfile
 *   conteúdo: name:pid:port:password:protocol (5 campos por ':')
 *   erros originais: "invalid lockfile port", "invalid lockfile pid",
 *   "lockfile must contain five colon-delimited fields" */

use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct Lockfile {
    pub name: String,
    pub pid: u32,
    pub port: u16,
    pub password: String,
}

pub fn read() -> Option<Lockfile> {
    let local = std::env::var("LOCALAPPDATA").ok()?;
    let path = PathBuf::from(local)
        .join("Riot Games")
        .join("Riot Client")
        .join("Config")
        .join("lockfile");
    let txt = std::fs::read_to_string(path).ok()?;
    parse(&txt)
}

fn parse(txt: &str) -> Option<Lockfile> {
    let fields: Vec<&str> = txt.trim().split(':').collect();
    if fields.len() != 5 {
        return None;
    }
    Some(Lockfile {
        name: fields[0].to_string(),
        pid: fields[1].parse().ok()?,
        port: fields[2].parse().ok()?,
        password: fields[3].to_string(),
    })
}
