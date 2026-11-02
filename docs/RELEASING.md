# Publicando uma release

Este é o processo usado para gerar e publicar o instalador do Crymora. Ele existe para garantir que o binário publicado, o checksum e o código-fonte estejam sempre em sincronia.

## Pré-requisitos

- Windows 10/11 x64 com Visual Studio C++ Build Tools.
- Rust estável com toolchain MSVC.
- Tauri CLI 2 (`cargo install tauri-cli --version "^2"`).
- Acesso de escrita ao repositório `nvd7/crymora`.

## 1. Atualizar a versão

A versão fica em dois lugares:

- `src-tauri/Cargo.toml` (`version = "0.1.2"`)
- `src-tauri/tauri.conf.json` (`"version": "0.1.2"`)

Mantenha os dois iguais e atualize o `CHANGELOG.md`.

## 2. Rodar os testes

```powershell
cd src-tauri
cargo test --lib
```

O CI repete os testes em `windows-latest` a cada push no `main`.

## 3. Gerar o instalador

```powershell
cd src-tauri
cargo tauri build
```

Saída:

```text
src-tauri\target\release\bundle\nsis\Crymora_<versão>_x64-setup.exe
```

## 4. Calcular o checksum

```powershell
$exe = "src-tauri\target\release\bundle\nsis\Crymora_0.1.2_x64-setup.exe"
(Get-FileHash $exe -Algorithm SHA256).Hash.ToLower()
(Get-Item $exe).Length
```

Atualize:

- `builds/final/Crymora_<versão>_x64-setup.exe` (copie o binário);
- `builds/final/SHA256SUMS.txt` (hash em minúsculas + dois espaços + nome do arquivo);
- `builds/final/LEIA-ME.txt` (tamanho, hash e data);
- o bloco de checksum no corpo da release.

## 5. Publicar no GitHub

1. Copie o instalador para `builds/final/`.
2. Faça commit e push dos arquivos atualizados.
3. Crie a tag e a release:

```powershell
git tag v0.1.2
git push origin v0.1.2
```

4. Anexe à release:

- `Crymora_<versão>_x64-setup.exe`
- `LEIA-ME.txt`
- `SHA256SUMS.txt`

5. Marque a release como **Latest** quando for a versão estável.

## 6. Verificar

- Baixe o asset publicado e confira o SHA-256 contra o arquivo local.
- Abra a página da release e confirme que o bloco de checksum e os links renderizam.
- Confirme que o CI do commit publicado está verde.
- Se o site (GitHub Pages) estiver ativo, confirme que a página inicial carrega.

## Assinatura de código

O projeto ainda não possui certificado de code signing. O instalador é publicado sem assinatura e pode acionar o SmartScreen. Quando houver certificado, assine o `.exe` antes de calcular o hash e anexar à release.
