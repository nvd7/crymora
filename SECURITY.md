# Política de segurança

## Versões suportadas

| Versão | Suporte |
| --- | --- |
| `0.1.x` | ✅ Recebe correções |
| `< 0.1` | ❌ Não suportada |

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
- Não envia dados para servidores próprios.
- O token local do Riot Client é usado apenas em memória, para chamadas ao próprio Riot, e não é gravado em log nem exposto à UI.
- Links externos são validados no backend e limitados a `tracker.gg` e `twitch.tv` em HTTPS.
- A UI roda com CSP restritiva e sem permissão de opener.
- Configurações aceitam apenas chaves booleanas conhecidas.

## Instalador não assinado

O instalador da versão `0.1.0` não possui assinatura de código. O SHA-256 de cada binário é publicado em `builds/final/SHA256SUMS.txt` e na release correspondente. Sempre confira o hash antes de executar.
