def _f(p):
    import os
    try:
        s = os.stat(p)
        print("d" if s[0] & 0x4000 else "f", s[6], sep="\t")
    except OSError:
        print("-")


_f({{path}})
del _f
