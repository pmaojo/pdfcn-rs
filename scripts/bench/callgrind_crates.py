#!/usr/bin/env python3
"""Share of a callgrind profile spent in each crate, by *self* cost.

Self (exclusive) instructions add up to the total, so each instruction is
counted once; a function's crate is the first path segment of its demangled
name (generic impls such as `<T as core::...>` are attributed to the crate of
the type when it is spelled first). See scripts/bench/README.md, section 4.

    callgrind_annotate --auto=no --threshold=99.9 c.out > c.txt
    python3 -I scripts/bench/callgrind_crates.py c.txt
"""
import re
import sys
from collections import Counter

# "  42,477,835 ( 4.05%)  /path/file.rs:crate::module::function [object]";
# file paths hold no ':', so the first one ends the file.
LINE = re.compile(r"^\s*([\d,]+)\s+\(\s*[\d.]+%\)\s+[^:\s]*:(.+?)(?:\s+\[[^\]]*\])?$")
GROUPS = {
    "azul": ("azul_", "azul"),
    "printpdf": ("printpdf",),
    "lopdf": ("lopdf",),
    "pdfcn": ("pdfcn_", "pdfcn"),
    "svg (usvg/resvg/svg2pdf/tiny_skia)": ("usvg", "resvg", "svg2pdf", "tiny_skia", "svgtypes", "pdf_writer"),
    "fonts (allsorts/ttf-parser/rust_fontconfig)": ("allsorts", "ttf_parser", "rust_fontconfig", "rustybuzz", "subsetter"),
    "images (image/png/jpeg/zune/fdeflate)": ("image", "png", "jpeg_decoder", "zune_", "fdeflate", "fast_image_resize"),
    "compression (flate2/miniz/zlib)": ("flate2", "miniz_oxide", "libz", "zlib", "adler"),
}


def crate_of(name):
    name = name.lstrip("<").lstrip("&").lstrip("*").replace("mut ", "").replace("const ", "")
    head = re.split(r"::|<| |\(", name, maxsplit=1)[0]
    for group, prefixes in GROUPS.items():
        if any(head == p or head.startswith(p) for p in prefixes):
            return group
    if head in ("core", "alloc", "std", "hashbrown"):
        return "std (hashing, collections, fmt)"
    if not head[:1].isalpha() or head.islower() and "_" in head and not "::" in name:
        return "libc (memcpy/memset/malloc)"
    return "other: " + head


def main(path):
    total, by = 0, Counter()
    in_table = False
    for raw in open(path, errors="replace"):
        if raw.startswith("Ir") and "file:function" in raw:
            in_table = True
            continue
        if raw.strip().endswith("PROGRAM TOTALS"):
            total = int(raw.split()[0].replace(",", ""))
            continue
        if not in_table:
            continue
        m = LINE.match(raw)
        if m:
            by[crate_of(m.group(2))] += int(m.group(1).replace(",", ""))
    other = sum(v for k, v in by.items() if k.startswith("other: "))
    print(f"total {total / 1e6:,.0f} M instructions")
    for k, v in sorted(by.items(), key=lambda kv: -kv[1]):
        if not k.startswith("other: ") or v > total * 0.01:
            print(f"{v / total * 100:5.1f} %  {k}")
    print(f"{other / total * 100:5.1f} %  other (all)")
    print(f"{(total - sum(by.values())) / total * 100:5.1f} %  below threshold")


if __name__ == "__main__":
    main(sys.argv[1])
