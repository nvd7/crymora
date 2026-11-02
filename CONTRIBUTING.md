# Contribuindo com o Crymora

Obrigado pelo interesse! Este documento explica como propor mudanças, reportar problemas e manter o projeto saudável.

## Antes de começar

Leia o [README](README.md), o [Código de Conduta](CODE_OF_CONDUCT.md) e a [Política de segurança](SECURITY.md).

### Princípio inegociável: somente leitura

O Crymora é um leitor passivo do Riot Client e dos logs locais. Contribuições que adicionem qualquer um dos itens abaixo **não serão aceitas**:

- leitura de memória do jogo;
- injeção de DLL, threads remotas ou hooks;
- overlay dentro do jogo;
- automação de input, macros ou bots;
- alteração de arquivos do Valorant;
- tentativa de contornar Vanguard, anti-cheat ou restrições da Riot;
- coleta oculta de dados ou telemetria.

Mudanças que aumentem a coleta de informações do cliente exigem discussão prévia e documentação explícita do risco para o usuário.

## Como contribuir

### Reportar um bug

Use o template de [bug report](.github/ISSUE_TEMPLATE/bug_report.yml). Inclua:

- versão do Crymora e do Windows;
- passos para reproduzir;
- comportamento esperado e observado;
- logs com Riot ID, puuid e tokens removidos.

### Sugerir uma funcionalidade

Use o template de [feature request](.github/ISSUE_TEMPLATE/feature_request.yml). Explique o problema que a ideia resolve, não apenas a solução proposta.

### Enviar código

1. Faça um fork e crie uma branch descritiva:
   ```powershell
   git checkout -b feat/minha-melhoria
   ```
2. Instale os pré-requisitos descritos no README.
3. Rode os testes:
   ```powershell
   cd src-tauri
   cargo test --lib
   ```
4. Mantenha o estilo do projeto:
   - Rust formatado com `cargo fmt`;
   - frontend em HTML/CSS/JS puro, sem framework e sem etapa de build;
   - textos novos em português **e** inglês no mapa de i18n;
   - comentários apenas onde o código não se explica sozinho.
5. Abra o PR usando o template e descreva o que mudou e como testou.

## Convenção de commits

Use [Conventional Commits](https://www.conventionalcommits.org/):

```text
feat: adiciona histórico de partidas no tracker
fix: corrige nick após troca de conta
docs: atualiza guia de instalação
build: atualiza instalador 0.1.2
refactor: simplifica o poller de presenças
test: cobre fallback de puuid no pré-jogo
```

## Dependências

As dependências são propostas pelo Dependabot (Cargo e GitHub Actions) e revisadas manualmente antes do merge. Atualizações que mudam o binário exigem um novo instalador e a atualização do checksum — veja [docs/RELEASING.md](docs/RELEASING.md). Alertas de segurança e correções automáticas de vulnerabilidades continuam habilitados no GitHub.

## Checklist do PR

- [ ] A mudança preserva o modelo somente leitura.
- [ ] `cargo test --lib` passa.
- [ ] O app foi testado com o Riot Client aberto, quando aplicável.
- [ ] Textos novos têm versão em português e inglês.
- [ ] Nenhum segredo, token, Riot ID ou log pessoal foi incluído.
- [ ] A documentação foi atualizada, se necessário.

## Publicar uma release

O processo completo — versão, testes, build, checksum, tag e assets — está em [docs/RELEASING.md](docs/RELEASING.md).

## Dúvidas

Abra uma discussão em [GitHub Discussions](https://github.com/nvd7/crymora/discussions) ou uma issue com a label `question`.
