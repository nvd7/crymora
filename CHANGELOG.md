# Changelog

Todas as mudanças relevantes do Crymora são documentadas neste arquivo.

O formato segue [Keep a Changelog](https://keepachangelog.com/pt-BR/1.1.0/) e o projeto adere ao [Versionamento Semântico](https://semver.org/lang/pt-BR/).

## [Não lançado]

### Planejado

- Histórico das últimas partidas no Tracker.
- Assinatura de código do instalador.
- Testes de integração com o Riot Client em CI.

## [0.1.0] - 2026-10-31

Primeira release pública do Crymora.

### Adicionado

- Aplicativo desktop para Windows com interface em HTML/CSS/JS e backend Rust + Tauri 2.
- **Home** com status do Riot Client, amigos online, partida atual e atalhos.
- **Partida** com pré-jogo e jogo ao vivo, incluindo os dois times, nick, rank, nível, mapa, modo, placar e marcas de party.
- **Amigos** com presença do cliente do Valorant, online primeiro, status, rank e nível.
- **Tracker** com busca local por Riot ID, perfil completo, rank atual e RR, pico de carreira, pico do ato, vitórias/derrotas, ato anterior, leaderboard, partidas na carreira e últimos resultados.
- **Ajustes** com tema, cor de destaque, densidade, animações, efeitos de fundo, idioma, iniciar com o Windows, alertas e presença no Discord.
- Ícone de bandeja com menu nativo e comportamento “fechar para a bandeja”.
- Instalador NSIS por usuário, sem necessidade de administrador.
- Fallback de avatar por iniciais para amigos sem card ou nível conhecido.
- Documentação de arquitetura, privacidade, segurança e contribuição.

### Corrigido

- Nick próprio e cache de amigos agora são refeitos ao trocar de conta, mesmo quando a sessão anterior cai para “offline” no meio da troca.
- Roster do pré-jogo agora completa o time inimigo pelo endpoint `loadouts` (que devolve o `Subject` dos 10 jogadores), já que o payload do Riot entrega apenas o time aliado. O `ShooterGame.log` fica como reserva e passou a ancorar na primeira consulta da partida, para não perder os inimigos no fim do agent-select.
- Puuid de jogadores agora usa `PlayerIdentity.Subject` quando o `Subject` superior vem nulo ou vazio.
- Rank para nick tampado usa `SeasonalBadgeInfo.Rank`; jogadores sem elo aparecem como “Unranked”.
- Nível 0 de presença offline não é mais exibido como nível real.

### Alterado

- Atualização imediata ao recuperar o foco (Alt+Tab), sem esperar o próximo ciclo.
- Polling contínuo de ~1,5 s no menu e ~1 s durante a partida, com consultas de elo em paralelo.
- O roster do pré-jogo só é montado quando todos os elos estão confirmados, com timeout de 5 s.
- Interface e textos revisados para deixar claro que o app é somente leitura.

### Segurança

- Nenhuma leitura de memória, injeção, hook, overlay, automação de input ou alteração de arquivos do jogo.
- Sem telemetria, backend próprio ou conta na nuvem.
- CSP restritiva, links externos limitados a `tracker.gg` e `twitch.tv` em HTTPS e whitelist de configurações.
- Token local do Riot Client usado apenas em memória e nunca gravado em log.

### Notas

- O instalador não é assinado digitalmente; o Windows SmartScreen pode exibir um aviso na primeira execução.
- Esta versão exibe os inimigos no pré-jogo. A Riot classifica “scouting” como uso não aprovado. Leia o aviso no README e use por sua conta e risco.
