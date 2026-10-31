# Privacidade

O Crymora foi feito para funcionar inteiramente na sua máquina. Esta página descreve exatamente quais dados o aplicativo acessa, onde eles ficam e o que sai do computador.

## Resumo

- **Nenhuma telemetria.**
- **Nenhum backend próprio.**
- **Nenhuma conta Crymora.**
- **Nenhum dado enviado para o desenvolvedor.**
- **Nenhum dado vendido ou compartilhado.**

## Dados acessados localmente

| Dado | Finalidade | Onde fica |
| --- | --- | --- |
| Lockfile do Riot Client | Descobrir porta e token da sessão local | Instalação do Riot Client |
| Token de sessão | Autenticar chamadas às APIs do próprio Riot | Apenas em memória |
| Presenças e amigos | Montar a lista de amigos | API local do Riot |
| Partida e roster | Mostrar pré-jogo e jogo ao vivo | API local do Riot |
| Elos e MMR | Exibir rank, RR, pico e estatísticas | API do Riot (PD) |
| `loadouts` do pré-jogo | Obter o `Subject` dos 10 jogadores | API local do Riot |
| `ShooterGame.log` | Reserva dos puuids inimigos no pré-jogo | Pasta de logs do Valorant |
| Configurações | Guardar preferências da UI | `%APPDATA%\crymora\settings.json` |
| Log de diagnóstico | Ajudar a diagnosticar problemas | `%APPDATA%\crymora\debug.log` |

## Conexões de rede

O Crymora se conecta apenas a:

1. **Riot Client local** (`127.0.0.1`) — APIs locais autenticadas com o token da sessão.
2. **Endpoints do Riot** (`glz-*.pvp.net`, `pd.*.pvp.net`) — partida, roster, nomes, elos e presenças, usando o token local.
3. **`valorant-api.com`** — CDN pública de ícones de ranks, agentes e mapas.
4. **Discord IPC local** — somente se a presença no Discord estiver ativada (desligada por padrão).

Links para **`tracker.gg`** e **`twitch.tv`** só são abertos quando você clica, no seu navegador padrão. O Crymora não faz requisições para esses sites.

## O que nunca é feito

- Nenhum acesso à memória do jogo.
- Nenhuma injeção de código, hook ou overlay.
- Nenhuma automação de input.
- Nenhuma alteração de arquivos do Valorant.
- Nenhum envio de token, Riot ID, puuid ou histórico para servidores do desenvolvedor.
- Nenhum analytics, crash reporting ou publicidade.

## Controle do usuário

- **Apagar configurações:** feche o app e remova `%APPDATA%\crymora\settings.json`.
- **Apagar logs:** remova `%APPDATA%\crymora\debug.log`.
- **Desinstalar:** use o desinstalador em `%LOCALAPPDATA%\Crymora\uninstall.exe` e, se quiser, apague a pasta `%APPDATA%\crymora`.

## Menores de idade

O Crymora não coleta dados pessoais e não possui interação social própria. Ainda assim, o uso do aplicativo está sujeito aos Termos de Serviço da Riot Games e à legislação aplicável.

## Alterações nesta política

Mudanças relevantes serão registradas no [CHANGELOG](../CHANGELOG.md). A versão publicada nesta página sempre corresponde à release mais recente do repositório.

## Contato

Dúvidas sobre privacidade: abra uma discussão em [GitHub Discussions](https://github.com/nvd7/crymora/discussions) ou entre em contato pelo perfil [@nvd7](https://github.com/nvd7).

---

**Última atualização:** 31 de outubro de 2026.
