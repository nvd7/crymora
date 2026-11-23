# Política de segurança

## Versões suportadas

| Versão | Suporte |
| --- | --- |
| `0.2.x` | ✅ Recebe correções |
| `< 0.2` | ❌ Não suportada |

## Como reportar uma vulnerabilidade

**Não abra uma issue pública para vulnerabilidades.**

Use o canal privado do GitHub:

1. Acesse a aba **Security** do repositório.
2. Clique em **Report a vulnerability**.
3. Descreva o problema, o impacto e os passos para reproduzir.

Se o canal privado não estiver disponível, entre em contato pelo perfil [@nvd7](https://github.com/nvd7) e peça um canal seguro antes de enviar detalhes.

### O que incluir

- Versão do Crymora e do Windows.
- Descrição do comportamento observado e do esperado.
- Passos para reproduzir, de preferência com uma prova de conceito mínima.
- Impacto estimado (ex.: execução de código, vazamento de token local, bypass de allowlist).
- Logs relevantes, **com Riot ID, puuid e tokens removidos**.

### O que esperar

- Confirmação de recebimento em até 7 dias.
- Avaliação e correção em prazo proporcional à gravidade.
- Crédito na release, se você quiser ser citado.

## Escopo

Dentro do escopo:

- Código do Crymora (Rust, frontend e instalador).
- Dependências empacotadas.
- CSP, allowlist de links externos, whitelist de settings e tratamento do token local.
- Qualquer caminho que permita ao app sair do modelo somente leitura.

Fora do escopo:

- Vulnerabilidades no Riot Client, no Valorant ou no Vanguard.
- Políticas da Riot Games, incluindo a classificação de “scouting”.
- Sites de terceiros (`tracker.gg`, `twitch.tv`, `valorant-api.com`).
- Problemas que exijam acesso físico ou conta já comprometida.

## Modelo de segurança

O Crymora é um leitor passivo:

- Não lê memória do jogo, não injeta código, não cria hooks ou overlay e não automatiza entrada.
- Não altera arquivos do Valorant.
- Não consulta `loadouts` nem lê o `ShooterGame.log` atrás de adversários: o pré-jogo mostra apenas o time aliado.
- Não envia dados para servidores próprios.
- O token local do Riot Client é usado apenas em memória, para chamadas ao próprio Riot, e não é gravado em log nem exposto à UI.
- Todo request passa por pacer com jitter e teto por minuto (`riot/limits.rs`); o `debug.log` redige PUUIDs.
- Links externos são validados no backend e limitados a `tracker.gg` e `twitch.tv` em HTTPS.
- A UI roda com CSP restritiva, `freezePrototype` e sem permissão de opener.
- Configurações aceitam apenas chaves booleanas conhecidas.

## Instalador não assinado

O instalador da versão `0.2.0` não possui assinatura de código. O SHA-256 de cada binário é publicado em `builds/final/SHA256SUMS.txt` e na release correspondente. Sempre confira o hash antes de executar.

## Avisos de dependência

A árvore é auditada contra o [OSV](https://osv.dev) e o [RustSec](https://rustsec.org) a cada release.

Estado da `0.2.0`:

| Crate | Aviso | Situação |
| --- | --- | --- |
| `cookie` | RUSTSEC-2026-0594 | Corrigido — 0.18.3 |
| `url` | RUSTSEC-2026-0592 | Corrigido — 2.5.9 |
| `rustix` | RUSTSEC-2026-0486 | Corrigido — 1.1.6 |
| `glib` | GHSA-wrw7-89jp-8q8g / RUSTSEC-2024-0429 | Não alcançável no Windows |

O `glib` é um aviso conhecido e **não afeta o binário do Windows**. Ele entra no `Cargo.lock` pela pilha Linux do Tauri (`gtk` 0.18 → `webkit2gtk` → `wry`), que só é compilada com `--target x86_64-unknown-linux-gnu`. O Crymora distribui apenas o instalador `x86_64-pc-windows-msvc`, então nenhum código do `glib` chega ao processo. A correção exige a migração da pilha gtk-rs para 0.20, que depende do Tauri e não pode ser feita daqui. O alerta correspondente fica registrado como *not affected* no Dependabot.

`chrono`, `proc-macro-error`, `zbus_names` e `zvariant` aparecem como *unmaintained* no RustSec, sem CVE associada e sem versão corrigida publicada. Também são de dependências transitivas.
