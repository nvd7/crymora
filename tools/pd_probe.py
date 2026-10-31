# Sonda via curl — dois focos:
#   1) name-service v3: o jogo usa, mas nos dá 401; testamos variações de
#      header (versão do client, entitlement) pra descobrir o que ele exige.
#      O v2 legado resolve só 4/11 (o resto vem com GameName vazio).
#   2) presences: dump de availability/product/last_online de cada amigo —
#      o app mostra todos como offline; o formato pode ter mudado.
#
# Uso:  python -I tools\pd_probe.py   (Riot Client aberto e logado;
#       idealmente com PELO MENOS 1 amigo online no momento da sonda)

import base64
import json
import subprocess
from pathlib import Path

def curl(args):
    r = subprocess.run(["curl.exe", "-s", *args], capture_output=True, text=True)
    return r.stdout + r.stderr

def read_lockfile():
    p = Path.home() / "AppData/Local/Riot Games/Riot Client/Config/lockfile"
    parts = p.read_text().strip().split(":")
    return parts[2], parts[3]

def split_status(out):
    return out.rsplit("HTTP", 1)[-1].strip(), out.rsplit("HTTP", 1)[0]

def main():
    port, password = read_lockfile()
    auth = base64.b64encode(f"riot:{password}".encode()).decode()

    raw_tok = curl([
        "-k", f"https://127.0.0.1:{port}/entitlements/v1/token",
        "-H", f"Authorization: Basic {auth}",
    ])
    try:
        tok = json.loads(raw_tok)
        access = tok["accessToken"]
        ent = tok.get("token") or tok.get("entitlementsToken")
        puuid = tok["subject"]
    except (json.JSONDecodeError, KeyError):
        print("[entitlements] resposta inesperada (Riot Client aberto e logado?):")
        print(raw_tok[:600])
        raise SystemExit(1)

    ver = None
    game_log = Path.home() / "AppData/Local/VALORANT/Saved/Logs/ShooterGame.log"
    for line in game_log.read_text(errors="replace").splitlines():
        if "CI server version: " in line:
            ver = line.split("CI server version: ", 1)[1].strip()
            break
    api_ver = json.loads(curl(["https://valorant-api.com/v1/version"]))["data"]
    print(f"jogo: {ver}")
    print(f"valorant-api: riotClientVersion={api_ver.get('riotClientVersion')} "
          f"riotClientBuild={api_ver.get('riotClientBuild')}\n")

    platform = base64.b64encode(
        b'{"platformType":"PC","platformOS":"Windows",'
        b'"platformOSVersion":"10.0.19042","platformChipset":"Unknown"}'
    ).decode()

    # 1) presences — formato novo (2026): sem availability/last_online
    pres = json.loads(curl([
        "-k", f"https://127.0.0.1:{port}/chat/v4/presences",
        "-H", f"Authorization: Basic {auth}",
    ]))["presences"]
    print(f"[presences] {len(pres)} entradas")
    for p in pres:
        print(f"  {str(p.get('puuid'))[:8]} product={p.get('product')!r} "
              f"state={p.get('state')!r} name={p.get('game_name')!r}")

    def dump(tag, entry):
        if not entry:
            print(f"[{tag}] nenhuma entrada")
            return
        d = dict(entry)
        if d.get("private"):
            try:
                d["private"] = json.loads(base64.b64decode(d["private"] + "==="))
            except Exception:
                pass
        print(f"[{tag}] {json.dumps(d, indent=1, ensure_ascii=False)[:1800]}")

    dump("presença valorant", next((p for p in pres if p.get("product") == "valorant"), None))
    dump("presença riot_client", next((p for p in pres if p.get("product") == "riot_client"), None))

    # 2) name-service v3 — variações de auth (PUT, corpo = [puuid])
    v3 = "https://pd.na.a.pvp.net/name-service/v3/players"
    variants = [
        ("v3 · ver do jogo", ver, True, "PUT"),
        ("v3 · riotClientVersion", api_ver.get("riotClientVersion"), True, "PUT"),
        ("v3 · riotClientBuild", api_ver.get("riotClientBuild"), True, "PUT"),
        ("v3 · sem entitlement", ver, False, "PUT"),
        ("v3 · POST ver do jogo", ver, True, "POST"),
    ]
    print()
    for label, v, with_ent, method in variants:
        h = ["-H", f"Authorization: Bearer {access}"]
        if with_ent:
            h += ["-H", f"X-Riot-Entitlements-JWT: {ent}"]
        h += [
            "-H", f"X-Riot-ClientVersion: {v}",
            "-H", f"X-Riot-ClientPlatform: {platform}",
        ]
        out = curl([*h, "-X", method, v3,
                    "-H", "Content-Type: application/json",
                    "-d", json.dumps([puuid]),
                    "-o", "-", "-w", "\nHTTP %{http_code}"])
        status, body = split_status(out)
        print(f"[{label}] HTTP {status}")
        print(f"  {body[:200]}".replace(chr(10), " "))

if __name__ == "__main__":
    main()
