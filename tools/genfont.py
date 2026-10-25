#!/usr/bin/env python3
"""
genfont.py — 生成 MimicryOS 的 8x16 PSF2 字体（font.psf）

字体数据来源：Spleen 8x16 1.0.4（BSD-2-Clause 许可，Frederic Cambus）
  https://github.com/Tecate/bitmap-fonts/blob/master/bitmap/spleen/spleen-8x16.bdf
本脚本下载该 BDF，解析为 256 个字形（0x00–0xFF 全覆盖，缺失编码填 0），
再打包成 PSF2 格式，供 objcopy 转成 font.o（kernel.c 依赖 _binary_font_psf_start）。

PSF2 头（32 字节，little-endian）：
  magic=0x864AB572  version=0  headersize=32  flags=0
  numglyph=256  bytesperglyph=16  height=16  width=8
"""
import struct
import sys
import urllib.request

BDF_URL = (
    "https://raw.githubusercontent.com/Tecate/bitmap-fonts/master/"
    "bitmap/spleen/spleen-8x16.bdf"
)
LOCAL_BDF = "spleen-8x16.bdf"   # tools/ 下的本地副本，优先使用
OUT = "font.psf"

def fetch_bdf(url, timeout=60):
    import os
    if os.path.exists(LOCAL_BDF):
        print(f"using local {LOCAL_BDF} ...", flush=True)
        with open(LOCAL_BDF, "r", encoding="utf-8") as f:
            return f.read()
    print(f"downloading {url} ...", flush=True)
    with urllib.request.urlopen(url, timeout=timeout) as r:
        return r.read().decode("utf-8")

def parse_bdf(text):
    """解析 BDF，返回 {encoding: bytes(16)}。"""
    glyphs = {}
    lines = text.splitlines()
    i = 0
    n = len(lines)
    while i < n:
        line = lines[i].strip()
        if line.startswith("ENCODING"):
            code = int(line.split()[1])
            # 前进到 BITMAP
            while i < n and not lines[i].strip().startswith("BITMAP"):
                i += 1
            i += 1  # 跳过 BITMAP
            rows = []
            while i < n and not lines[i].strip().startswith("ENDCHAR"):
                tok = lines[i].strip()
                if tok:
                    rows.append(int(tok, 16))
                i += 1
            # 取 16 行（BDF 每字符固定 8x16）
            data = bytes(rows[:16]) if len(rows) >= 16 else bytes(rows) + b"\x00" * (16 - len(rows))
            glyphs[code] = data
        i += 1
    return glyphs

def build_psf2(glyphs):
    # 256 字形全覆盖，缺失编码填 0（空格）
    glyph_table = bytearray()
    for code in range(256):
        glyph_table += glyphs.get(code, b"\x00" * 16)
    header = struct.pack("<8I", 0x864AB572, 0, 32, 0, 256, 16, 16, 8)
    assert len(header) == 32, f"header size = {len(header)}"
    return header + bytes(glyph_table)

def main():
    text = fetch_bdf(BDF_URL)
    glyphs = parse_bdf(text)
    if not glyphs:
        print("no glyphs parsed", file=sys.stderr)
        sys.exit(1)
    min_c, max_c = min(glyphs), max(glyphs)
    print(f"parsed {len(glyphs)} glyphs, code range 0x{min_c:02x}..0x{max_c:02x}", flush=True)
    data = build_psf2(glyphs)
    with open(OUT, "wb") as f:
        f.write(data)
    print(f"wrote {OUT}: {len(data)} bytes (PSF2, 256 glyphs, 8x16)", flush=True)

if __name__ == "__main__":
    main()
