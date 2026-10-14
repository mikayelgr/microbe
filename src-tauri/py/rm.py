def _f(p):
    import os

    def r(q):
        if os.stat(q)[0] & 0x4000:
            for e in os.ilistdir(q):
                r(q.rstrip("/") + "/" + e[0])
            os.rmdir(q)
        else:
            os.remove(q)

    r(p)


_f({{path}})
del _f
