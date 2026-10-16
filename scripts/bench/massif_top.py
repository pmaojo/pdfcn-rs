"""Summarises a massif.out peak snapshot by the first frame of interest.

    valgrind --tool=massif --massif-out-file=m.out target/profiling/pdfcn build ...
    python3 -I scripts/bench/massif_top.py m.out [--depth 3]

Allocator, `alloc::`/`core::`/`std::` and inlined-unknown frames are skipped,
so each line is "who asked for the bytes", not "which Vec grew".
"""
import re, sys

SKIP = re.compile(r"(alloc|__rdl_alloc|__rust_|realloc|malloc|UnknownInlinedFun|"
                  r"raw_vec|RawVec|reserve|with_capacity|grow_|finish_grow|do_reserve|"
                  r"<alloc::|alloc::|core::|std::|hashbrown|clone|to_vec|extend|push|"
                  r"from_iter|collect|SpecFromIter|SpecExtend|into_vec)", re.I)


def parse(path):
    lines = open(path).read().split("\n")
    snaps, cur = [], None
    i = 0
    while i < len(lines):
        l = lines[i]
        if l.startswith("snapshot="):
            cur = {"tree": None}
            snaps.append(cur)
        elif l.startswith("mem_heap_B="):
            cur["heap"] = int(l.split("=")[1])
        elif l.startswith("heap_tree=") and l.split("=")[1] in ("peak", "detailed"):
            cur["kind"] = l.split("=")[1]
            cur["tree"], i = read_tree(lines, i + 1)
            continue
        i += 1
    return snaps


def read_tree(lines, i):
    m = re.match(r"\s*n(\d+): (\d+) (.*)", lines[i])
    n, size, label = int(m.group(1)), int(m.group(2)), m.group(3)
    node = {"size": size, "label": label, "kids": []}
    i += 1
    for _ in range(n):
        kid, i = read_tree(lines, i)
        node["kids"].append(kid)
    return node, i


def fn_name(label):
    m = re.match(r"0x[0-9A-F]+: (.*?) \(", label)
    return m.group(1) if m else label[:80]


def interesting(node, acc, path):
    name = fn_name(node["label"])
    if node["kids"] and (SKIP.search(name) or name.startswith("0x")):
        for k in node["kids"]:
            interesting(k, acc, path)
        return
    key = " <- ".join(path + [name]) if path else name
    if len(path) + 1 < DEPTH and node["kids"]:
        for k in node["kids"]:
            interesting(k, acc, path + [name])
        accounted = sum(k["size"] for k in node["kids"])
        if node["size"] > accounted:
            acc[key + " (self)"] = acc.get(key + " (self)", 0) + node["size"] - accounted
    else:
        acc[key] = acc.get(key, 0) + node["size"]


DEPTH = 2
if __name__ == "__main__":
    path = sys.argv[1]
    if "--depth" in sys.argv:
        DEPTH = int(sys.argv[sys.argv.index("--depth") + 1])
    snaps = parse(path)
    peak = next(s for s in snaps if s.get("kind") == "peak")
    print(f"peak heap: {peak['heap'] / 1e6:.1f} MB")
    acc = {}
    for k in peak["tree"]["kids"]:
        interesting(k, acc, [])
    for key, size in sorted(acc.items(), key=lambda kv: -kv[1])[:25]:
        if size > 0.5e6:
            print(f"{size / 1e6:8.1f} MB  {key}")
