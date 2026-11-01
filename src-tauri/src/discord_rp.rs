/* Discord Rich Presence — presença "Jogando Crymora" com mapa e placar,
 * formato pedido pelo usuário:
 *   detalhes = mapa · modo    estado = placar (ex: 7 – 5)
 *   tempo = desde que o Crymora abriu
 *   botão = GitHub do projeto
 * Conexão é 100% local (pipe \\.\pipe\discord-ipc-N); o cliente Discord
 * do usuário cuida do resto. Nada do jogo é lido por aqui. */

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};
use std::time::{SystemTime, UNIX_EPOCH};

/* Application ID do app "Crymora" no Discord Developer Portal.
 * O ID é público (não é segredo) e identifica nome, logo e assets. */
pub const DISCORD_APP_ID: &str = "1566333446446649374";

const GITHUB_URL: &str = "https://github.com/nvd7/crymora";

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub struct DiscordRp {
    client: Option<DiscordIpcClient>,
    last_sig: String,
    last_error: String,
    started_at: i64,
}

impl DiscordRp {
    pub fn new() -> Self {
        Self {
            client: None,
            last_sig: String::new(),
            last_error: String::new(),
            started_at: unix_now(),
        }
    }

    pub fn update(&mut self, enabled: bool, phase: &str, details: &str, state: &str) {
        if !enabled {
            if let Some(mut c) = self.client.take() {
                let _ = c.close();
            }
            self.last_sig.clear();
            return;
        }

        /* nada mudou e o pipe continua de pé → não refaz a activity */
        let sig = format!("{phase}|{details}|{state}");
        if sig == self.last_sig && self.client.is_some() {
            return;
        }

        if self.client.is_none() {
            match DiscordIpcClient::new(DISCORD_APP_ID) {
                Ok(mut c) => {
                    if c.connect().is_err() {
                        return; // Discord fechado — tenta de novo no próximo tick
                    }
                    self.client = Some(c);
                }
                Err(_) => return,
            }
        }
        let Some(client) = self.client.as_mut() else { return };

        let mut act = activity::Activity::new()
            .details(if details.is_empty() { "Crymora" } else { details })
            .timestamps(activity::Timestamps::new().start(self.started_at))
            .assets(
                activity::Assets::new()
                    .large_image("crymora_logo")
                    .large_text("Crymora · made by nvd7"),
            )
            .buttons(vec![activity::Button::new("GitHub", GITHUB_URL)]);
        if !state.is_empty() {
            act = act.state(state);
        }

        match client.set_activity(act) {
            Ok(()) => {
                self.last_sig = sig;
                self.last_error.clear();
            }
            Err(e) => {
                /* Discord pode ter fechado/trocado de pipe: derruba e
                 * tenta reconectar no próximo tick, sem spammar o log. */
                let msg = e.to_string();
                if msg != self.last_error {
                    crate::clog!("[discord] falha ao atualizar presence: {msg}");
                    self.last_error = msg;
                }
                self.client = None;
                self.last_sig.clear();
            }
        }
    }
}

impl Drop for DiscordRp {
    fn drop(&mut self) {
        if let Some(mut c) = self.client.take() {
            let _ = c.close();
        }
    }
}
