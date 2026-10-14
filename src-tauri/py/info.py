def _f(root):
    import sys, gc
    try:
        import os
    except ImportError:
        import uos as os

    def has(n):
        try:
            __import__(n)
            return 1
        except ImportError:
            return 0

    i = sys.implementation
    u = os.uname() if hasattr(os, "uname") else None
    v = ".".join(str(x) for x in i.version[:3])
    if len(i.version) > 3 and i.version[3]:
        v += "-" + i.version[3]
    gc.collect()
    try:
        s = os.statvfs(root)
        fs = (s[0] * s[2], s[0] * s[3])
    except Exception:
        fs = (0, 0)
    try:
        mf, ma = gc.mem_free(), gc.mem_alloc()
    except Exception:
        mf, ma = 0, 0
    mods = ("network", "bluetooth", "webrepl", "hashlib", "binascii", "mip", "deflate",
            "machine", "_thread", "asyncio", "json", "os", "time", "framebuf", "neopixel", "esp32")
    for k, val in (
        ("platform", sys.platform),
        ("version", v),
        ("machine", getattr(i, "_machine", u.machine if u else "")),
        ("build", getattr(i, "_build", "")),
        ("mpy", getattr(i, "_mpy", 0)),
        ("release", u.release if u else ""),
        ("mem_free", mf),
        ("mem_alloc", ma),
        ("fs_total", fs[0]),
        ("fs_free", fs[1]),
        ("modules", ",".join(n for n in mods if has(n))),
    ):
        print(k, val, sep="\t")


_f({{root}})
del _f
