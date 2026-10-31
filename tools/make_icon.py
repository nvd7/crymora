#!/usr/bin/env python3
"""Gera icons/icon.ico + icon.png do Crymora em Python puro (sem PIL).

Desenho: quadrado arredondado escuro com um "C" em anel (gap à direita),
gradiente vermelho->verde-menta (as duas cores de acento do app).
Renderiza em 4x supersampling com cobertura SDF p/ anti-alias.

Saídas:
  src-tauri/icons/icon.ico  (16/32/48/64/128/256 BMP 32bpp + 256 PNG)
  src-tauri/icons/icon.png (1024x1024, p/ asset do Discord / cargo tauri icon)
"""
import math
import os
import struct
import zlib

OUT_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src-tauri", "icons")

# ---------- desenho ----------

def sd_round_rect(x, y, hx, hy, r):
    qx = abs(x) - (hx - r)
    qy = abs(y) - (hy - r)
    ax = max(qx, 0.0)
    ay = max(qy, 0.0)
    return math.hypot(ax, ay) + min(max(qx, qy), 0.0) - r


def render(size, ss=4):
    """Devolve lista de linhas RGBA (top-down), bytes por pixel."""
    n = size * ss
    half = n / 2.0
    rows = []
    for py in range(size):
        row = bytearray()
        for px in range(size):
            # acumula subpixels
            r = g = b = a = 0.0
            for sy in range(ss):
                for sx in range(ss):
                    x = (px * ss + sx + 0.5) - half
                    y = (py * ss + sy + 0.5) - half
                    # fundo: quadrado arredondado (92% da área)
                    d_bg = sd_round_rect(x, y, half * 0.92, half * 0.92, half * 0.34)
                    cov_bg = min(max(0.5 - d_bg, 0.0), 1.0)
                    # anel do "C"
                    r_mid = half * 0.52
                    half_w = half * 0.145
                    dist = math.hypot(x, y)
                    d_ring = abs(dist - r_mid) - half_w
                    ang = math.atan2(y, x)          # gap à direita (ang ~ 0)
                    gap_half = math.radians(29.0)
                    # distancia arco até a borda do gap (negativo = dentro do C)
                    m_ang = (abs(ang) - gap_half) * dist
                    d_c = max(d_ring, -m_ang)
                    cov_c = min(max(0.5 - d_c, 0.0), 1.0)
                    if cov_bg + cov_c <= 0.0:
                        continue
                    # cores
                    t = (y / half + 1.0) * 0.5        # 0 topo, 1 base
                    # fundo: azul-escuro quase preto
                    bg = (0x14 - t * 0x0A, 0x14 - t * 0x0A, 0x1E - t * 0x10)
                    # anel: vermelho -> menta
                    cr = 0xFF + (0x3B - 0xFF) * t
                    cg = 0x46 + (0xF0 - 0x46) * t
                    cb = 0x55 + (0xC4 - 0x55) * t
                    # compõe anel sobre fundo sobre transparente
                    out_r = cr * cov_c + bg[0] * cov_bg * (1.0 - cov_c)
                    out_g = cg * cov_c + bg[1] * cov_bg * (1.0 - cov_c)
                    out_b = cb * cov_c + bg[2] * cov_bg * (1.0 - cov_c)
                    out_a = cov_bg + cov_c * (1.0 - cov_bg)
                    if out_a > 0.0:
                        r += out_r / out_a * out_a
                        g += out_g / out_a * out_a
                        b += out_b / out_a * out_a
                        a += out_a
            k = ss * ss
            row += bytes((int(r / k + 0.5) & 0xFF, int(g / k + 0.5) & 0xFF,
                          int(b / k + 0.5) & 0xFF, int(a / k + 0.5) & 0xFF))
        rows.append(bytes(row))
    return rows


# ---------- PNG ----------

def write_png(path, rows, size):
    def chunk(tag, data):
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))
    raw = b"".join(b"\x00" + r for r in rows)
    png = (b"\x89PNG\r\n\x1a\n"
           + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
           + chunk(b"IDAT", zlib.compress(raw, 9))
           + chunk(b"IEND", b""))
    with open(path, "wb") as f:
        f.write(png)


# ---------- ICO ----------

def bmp_entry(size, rows):
    """Entrada BMP 32bpp (BGRA bottom-up + máscara AND vazia)."""
    hdr = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0,
                      0, 0, 0, 0, 0)
    body = b"".join(bytes((px[2], px[1], px[0], px[3]))
                    for row in reversed(rows) for px in
                    (row[i:i + 4] for i in range(0, len(row), 4)))
    and_mask = b"\x00" * (((size + 31) // 32) * 4 * size)
    return hdr + body + and_mask


def png_entry(size, rows):
    def chunk(tag, data):
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))
    raw = b"".join(b"\x00" + r for r in rows)
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(raw, 9))
            + chunk(b"IEND", b""))


def write_ico(path, entries):
    """entries: [(size, bytes)] — 256 usa PNG, resto BMP."""
    n = len(entries)
    head = struct.pack("<HHH", 0, 1, n)
    dir = b""
    blobs = b""
    off = 6 + 16 * n
    for size, data in entries:
        w = size if size < 256 else 0
        dir += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(data), off)
        blobs += data
        off += len(data)
    with open(path, "wb") as f:
        f.write(head + dir + blobs)


def main():
    os.makedirs(OUT_DIR, exist_ok=True)
    # 256 (png-in-ico + base p/ menores) a partir de render direto
    rows256 = render(256)
    write_ico(os.path.join(OUT_DIR, "icon.ico"),
              [(256, png_entry(256, rows256))]
              + [(s, bmp_entry(s, render(s))) for s in (16, 32, 48, 64, 128)])
    write_png(os.path.join(OUT_DIR, "icon.png"), render(1024, ss=2), 1024)
    for name in ("icon.ico", "icon.png"):
        p = os.path.join(OUT_DIR, name)
        print(f"{name}: {os.path.getsize(p)} bytes")


if __name__ == "__main__":
    main()
