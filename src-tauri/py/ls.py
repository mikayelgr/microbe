def _f(p):
    import os
    for e in os.ilistdir(p):
        d = e[1] & 0x4000
        n = e[3] if len(e) > 3 else (0 if d else os.stat(p.rstrip("/") + "/" + e[0])[6])
        print(e[0], "d" if d else "f", n, sep="\t")


_f({{path}})
del _f
