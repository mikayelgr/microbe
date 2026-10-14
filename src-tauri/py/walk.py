def _f(p, with_hash):
    import os
    try:
        import hashlib, binascii
    except ImportError:
        hashlib = None
    buf = bytearray(512)
    mv = memoryview(buf)

    def h(f):
        if not hashlib:
            return "-"
        s = hashlib.sha256()
        with open(f, "rb") as fp:
            while True:
                n = fp.readinto(buf)
                if not n:
                    break
                s.update(mv[:n])
        return binascii.hexlify(s.digest()).decode()

    def w(d):
        for e in os.ilistdir(d):
            q = d.rstrip("/") + "/" + e[0]
            if e[1] & 0x4000:
                print(q, "d", 0, "-", sep="\t")
                w(q)
            else:
                print(q, "f", e[3] if len(e) > 3 else os.stat(q)[6], h(q) if with_hash else "-", sep="\t")

    w(p)


_f({{path}}, {{hash}})
del _f
