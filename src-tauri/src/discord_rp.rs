/* Discord Rich Presence — presença "Jogando Crymora" com mapa e placar,
 * formato pedido pelo usuário:
 *   detalhes = modo · mapa    estado = placar (ex: 7–5)
 * Conexão é 100% local (pipe \\?\pipe\discord-ipc-N); o cliente Discord
 * do usuário cuida do resto. */

use discord_rich_presence::{activity, DiscordIpc, DiscordIpcClient};

/* Application ID do app "Crymora" que VOCÊ cria em
 * https://discord.com/developers/applications → cole o ID aqui.
 * Sem ele, a presence não conecta (o ID do original pertence a eles). */
pub const DISCORD_APP_ID: &str = "COLE_SEU_APP_ID_AQUI";

pub struct DiscordRp {
    client: Option<DiscordIpcClient>,
    last_sig: String,
}

impl DiscordRp {
    pub fn new() -> Self {
        Self { client: None, last_sig: String::new() }
    }

    pub fn update(&mut self, enabled: bool, phase: &str, details: &str, state: &str) {
        if !enabled {
            if let Some(mut c) = self.client.take() {
                let _ = c.close();
            }
            return;
        }
        /* nada mudou → não refaz a activity a cada tick */
        let sig = format!("{phase}|{details}|{state}");
        if sig == self.last_sig {
            return;
        }
        self.last_sig = sig;

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
            .state(state);
        act = act.assets(
            activity::Assets::new()
                .large_image("crymora_logo")
                .large_text("Crymora"),
        );
        let _ = client.set_activity(act);
    }
}

impl Drop for DiscordRp {
    fn drop(&mut self) {
        if let Some(mut c) = self.client.take() {
            let _ = c.close();
        }
    }
}
