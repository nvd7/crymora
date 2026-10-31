# Crymora

Companheiro local do Valorant — rebuild próprio (fase 2). O backend Rust
foi escrito do zero com base na engenharia reversa do binário original
(evidência documentada nos cabeçalhos de `src-tauri/src/*.rs`).

Conexões externas: **só** Riot (API local + glz/pd) e valorant-api.com
(CDN público de ícones). Sem backend próprio, sem telemetria, sem store.

## Estrutura

```
crymora/
  ui/               frontend (mesma UI da edição patcheada)
    index.html
    app.js          ← copiar de ascend-mod\newui\
    styles.css      ← copiar de ascend-mod\newui\
    assets/         ← fontes woff2 (copiar, ver abaixo)
  src-tauri/        backend Rust
    tauri.conf.json
    capabilities/default.json
    src/
      main.rs       entrada
      lib.rs        comandos IPC + boot
      settings.rs   settings.json
      discord_rp.rs presence do Discord
      riot/         lockfile, região, HTTP, poll loop
```

## Pré-requisitos (uma vez só)

1. **Rust + MSVC** — https://rustup.rs (instala o Visual Studio C++
   Build Tools quando pedir, marque "Desktop development with C++").
2. **Tauri CLI**:
   ```
   cargo install tauri-cli --version "^2"
   ```
3. **WebView2** — já vem no Windows 11 (o app original usava o mesmo).

## Preparar o frontend (cmd)

```
copy D:\pasta\ascend-mod\newui\app.js    D:\pasta\crymora\ui\app.js
copy D:\pasta\ascend-mod\newui\styles.css D:\pasta\crymora\ui\styles.css
mkdir D:\pasta\crymora\ui\assets 2>nul
copy D:\pasta\ascend-mod\extracted\assets\geist-latin-wght-normal-BgDaEnEv.woff2  D:\pasta\crymora\ui\assets\
copy D:\pasta\ascend-mod\extracted\assets\geist-mono-latin-wght-normal-B_7UjwxQ.woff2 D:\pasta\crymora\ui\assets\
```

## Discord presence (opcional, uma vez só)

1. https://discord.com/developers/applications → **New Application**
   → nomeie **Crymora** (é esse nome que aparece no Discord).
2. Rich Presence → copie o **Application ID**.
3. Cole em `src-tauri\src\discord_rp.rs`, na constante `DISCORD_APP_ID`
   (trocando `COLE_SEU_APP_ID_AQUI`).
4. Na página do app: Assets → envie uma imagem chamada `crymora_logo`
   (o logo que aparecerá na presence).

O toggle fica nas configurações do Crymora (discordRpEnabled), desligado
por padrão. Formato: `mapa · modo` + placar ao vivo.

## Ícone do app

Gerado por script próprio (sem depender do tauri-cli):
```
python tools\make_icon.py
```
Isso escreve `src-tauri\icons\icon.ico` (o `tauri.conf.json` aponta pra
ele — é o ícone do exe, da janela e da bandeja) e `icons\icon.png`
(1024×1024 — reutilize como asset `crymora_logo` do Discord).

## Rodar / compilar

O frontend é estático (`frontendDist: ../ui`), então **não precisa do
tauri-cli** — o cargo resolve tudo:

```
cd /d D:\pasta\crymora\src-tauri
cargo run             ← testar
cargo build --release ← exe final em target\release\crymora.exe
```

Instalador NSIS (opcional, exige tauri-cli): `cargo install tauri-cli
--version "^2"` e `cargo tauri build`.

## Notas honestas

- **Menu de contexto (botão direito na janela)**: Configurações, Idioma,
  Recarregar, Minimizar e **Sair do Crymora**. O `quit_app` existe no
  binário original (era o "Quit" do TrayMenu deles) e no rebuild — nos
  dois, "Sair" encerra o processo de verdade.
- **Bandeja**: o rebuild tem ícone de bandeja com menu nativo
  (Abrir Crymora / Sair do Crymora); o X da janela esconde pra bandeja
  (mesmo comportamento do original — "hidden-to-tray"). Sair de verdade
  = menu da bandeja ou quit_app.
- No exe patcheado, o popup da bandeja (janela "tray-menu") carrega a
  UI nova: menu compacto com status do Riot, toggle de presence,
  Abrir e Sair — comandos originais reaproveitados.

- **form[] (últimos 5 resultados)**: ainda não implementado no rebuild
  (precisa do match-history do pd + detalhes por partida). A UI degrada
  bem: a linha simplesmente não aparece. Está marcado como TODO.
- O `tracker_proxy` continua stub (fase 2: webview escondida pro
  tracker.gg). Botões do Twitch degradam graciosamente.
- Binário sem assinatura digital → Smart Screen mostra aviso na
  primeira execução (igual qualquer app novo sem certificado).
