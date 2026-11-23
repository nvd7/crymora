# Arquitetura

Este documento descreve como o Crymora é organizado, de onde vêm os dados e quais são os limites de segurança do projeto.

## Visão geral

O Crymora é um aplicativo desktop Tauri 2 com duas camadas:

- **Backend em Rust** (`src-tauri/src`): descobre o Riot Client, autentica nas APIs locais, faz polling, normaliza os dados e expõe comandos IPC.
- **Frontend estático** (`ui/`): HTML, CSS e JavaScript puro, sem framework, sem bundler e sem npm. É servido diretamente pelo Tauri a partir de `frontendDist: ../ui`.

Não existe servidor intermediário. O binário fala diretamente com o Riot Client local e com endpoints do próprio Riot.

```mermaid
flowchart LR
    subgraph Machine["Sua máquina"]
        RC["Riot Client\nlockfile + APIs locais"]
        LOG["ShooterGame.log"]
        APP["Crymora\nRust + Tauri 2"]
        UI["UI estática\nHTML/CSS/JS"]
        RC -->|"lockfile, glz, pd"| APP
        LOG -->|"leitura"| APP
        APP -->|"IPC"| UI
    end
    CDN["valorant-api.com\nícones e mapas"]
    WEB["tracker.gg / twitch.tv\nnavegador padrão"]
    CDN -->|"HTTPS público"| UI
    UI -.->|"links allowlisted"| WEB
```

## Componentes do backend

| Arquivo | Responsabilidade |
| --- | --- |
| `main.rs` | Entrada do executável. |
| `lib.rs` | Bootstrap do Tauri, comandos IPC, tray, janela e allowlist de links. |
| `settings.rs` | Leitura/escrita de `settings.json` com whitelist de chaves booleanas. |
| `discord_rp.rs` | Rich Presence opcional no Discord. |
| `riot/lockfile.rs` | Descoberta do Riot Client e leitura do lockfile local. |
| `riot/region.rs` | Detecção de região, shard e host a partir do lockfile/ambiente. |
| `riot/remote.rs` | Cliente HTTP para glz/pd, nomes, MMR e normalização de ranks. |
| `riot/limits.rs` | Pacer (intervalo + jitter) e teto de requisições por minuto. |
| `riot/state.rs` | Poller, cache, montagem do roster aliado e presenças. |

### Comandos IPC

O frontend só acessa o backend por comandos explícitos:

- `get_match_state`, `get_pregame`, `get_live_roster`, `get_friends`, `riot_status`
- `refresh_now`
- `open_external`
- `settings_get`, `settings_set`
- `quit_app`, `open_main_window`
- `js_diag` (diagnóstico)

## Fluxo de dados

### 1. Descoberta e autenticação

1. O backend localiza o lockfile do Riot Client.
2. Extrai porta, token e protocolo.
3. Detecta região/shard para montar os hosts `glz-*` e `pd.*`.
4. Usa o token local em memória para chamadas autenticadas ao próprio Riot.

O token nunca é gravado em log, nunca é enviado à UI e nunca sai para servidores de terceiros.

### 2. Polling

O `Poller` mantém um cache em memória e executa `tick()` em loop:

- **Menu:** intervalo de 1,5 s (`TICK_IDLE_MS`).
- **Pré-jogo / partida:** intervalo de 1 s (`TICK_ACTIVE_MS`).
- **Cache de MMR:** TTL de 10 minutos (`MMR_TTL_SECS`).
- **Foco:** a UI chama `refresh_now` ao receber `focus` ou `visibilitychange`, forçando um tick imediato.

Cada tick coleta presenças, fase da partida, roster e dados de elo. As consultas de MMR passam por um orçamento de 2 jogadores por tick, cada uma espaçada pelo `riot/limits.rs`; o resto resolve nos ticks seguintes (cache de 10 min).

### 3. Pré-jogo — só o time aliado

O payload de `Pregame_GetMatch` entrega apenas o time aliado: `Teams` contém um único time e `EnemyTeam` vem nulo. O backend descobre o `TeamID` do próprio puuid e mantém **apenas** esse time no roster. Nada mais é consultado.

> **Por que não há busca de inimigos.** Até a 0.1.2 o app consultava
> `GET /pregame/v1/matches/{id}/loadouts` (que devolve o `Subject` dos 10) e
> varria o `ShooterGame.log` atrás de `Subject had empty display name. <uuid>`
> para completar o time adversário. A Riot classifica revelar o time inimigo
> antes da partida como *scouting* — uso não aprovado, sujeito a banimento.
> Os dois caminhos foram removidos na 0.2.0: `enemies_from_loadouts`,
> `enemies_from_log` e `parse_enemy_puuids` não existem mais, e o snapshot
> carrega `enemiesHidden: true` durante o pré-jogo para a UI explicar a
> ausência em vez de mostrar uma seção vazia.

```mermaid
sequenceDiagram
    participant UI
    participant Poller
    participant GLZ as Riot glz
    participant PD as Riot pd
    UI->>Poller: refresh_now (foco)
    Poller->>GLZ: GET pregame match
    GLZ-->>Poller: time aliado (EnemyTeam ignorado)
    Poller->>PD: nomes + MMR (2 por tick, com pacer)
    PD-->>Poller: nick, rank, pico
    Poller-->>UI: roster 5/5 + enemiesHidden
```

### 4. Espera pelos ranks

O roster do pré-jogo só é publicado quando `ranks_ready` confirma que todos os jogadores visíveis têm elo resolvido — “unranked” confirmado também conta. Se alguma consulta falhar, `pregame_hold` libera a tela após 3 s (`RANK_WAIT_SECS`) com o que estiver disponível.

Isso evita a UI “piscar” com ranks incompletos e mantém a promessa de “deu Alt+Tab, já está tudo lá”.

### 5. Core-game

Durante a partida, o core-game já retorna os 10 jogadores em `Players`, cada um com `TeamID`. O backend identifica o time do jogador local e normaliza aliados e inimigos. Nomes ausentes são resolvidos em lote e ficam em cache.

## Frontend

O frontend é deliberadamente simples:

- `index.html` define as views (Home, Partida, Amigos, Tracker, Ajustes).
- `app.js` contém estado, i18n (pt-BR/en-US), renderização, busca local e integração IPC.
- `styles.css` concentra temas, densidade, animações e layout.
- `assets/` contém as fontes Geist e o logo.

A UI não acessa rede diretamente, exceto `valorant-api.com` para ícones e mapas, permitido pela CSP. Links externos passam pelo comando `open_external`, que valida o host no backend.

## Segurança

### Limites do modelo

- **Somente leitura:** nenhuma escrita em arquivos do jogo, nenhuma memória, nenhuma injeção.
- **Sem overlay:** a janela é um app desktop comum.
- **Sem automação:** nenhum input sintético.
- **Sem telemetria:** nenhum backend próprio.
- **Dados locais:** `%APPDATA%\crymora\settings.json` e `debug.log`.

### Controles técnicos

- CSP restritiva em `tauri.conf.json` (`default-src 'self'`, `object-src 'none'`, `form-action 'none'`, `frame-src`/`frame-ancestors`/`worker-src` em `none`).
- `freezePrototype` habilitado no webview.
- `connect-src` limitado a IPC e `https://valorant-api.com`.
- `img-src` limitado a `self`, `data:` e `https://media.valorant-api.com`.
- Links externos allowlisted no Rust: apenas `tracker.gg` e `twitch.tv` em HTTPS.
- Settings com whitelist de chaves e valores booleanos; o restante é descartado.
- Nenhuma permissão de opener para a página.
- Todo request passa por `riot/limits.rs` (pacer + jitter + teto/minuto) e o `debug.log` redige PUUIDs.

### Riscos conhecidos

- **Scouting (removido):** até a 0.1.2 o app revelava o time inimigo no pré-jogo, o que a Riot classifica como uso não aprovado. O fluxo foi removido na 0.2.0 — não há mais consulta a `loadouts` nem leitura do `ShooterGame.log` atrás de adversários.
- **Instalador não assinado:** sujeito ao SmartScreen até haver certificado de code signing.
- **APIs não oficiais:** endpoints locais do Riot Client podem mudar sem aviso e quebrar funcionalidades.

## Testes

Os testes unitários ficam em `src-tauri/src` e cobrem:

- parsing de região;
- parsing de MMR e pico de carreira;
- fallback de puuid via `PlayerIdentity.Subject`;
- `ranks_ready` e timeout de `pregame_hold`;
- redação de PUUIDs no `debug.log`;
- allowlist de links externos;
- faixa do jitter do pacer.

```powershell
cd src-tauri
cargo test --lib
```

Testes que exigem o Riot Client aberto são marcados como `ignored` e podem ser executados manualmente com variáveis de ambiente específicas.

## Decisões de projeto

- **Rust + Tauri 2** por gerar um binário leve, sem runtime Node e com WebView2 já presente no Windows.
- **Frontend sem framework** para manter o bundle pequeno e o build reprodutível.
- **Polling curto com atualização sob demanda** em vez de websockets, porque as APIs locais do Riot não expõem um canal estável para todos os dados necessários.
- **Cache em memória** para MMR e nomes, reduzindo chamadas e mantendo a UI responsiva.
- **Degradação graciosa:** qualquer falha de rede ou de API resulta em estado parcial, nunca em tela travada.
