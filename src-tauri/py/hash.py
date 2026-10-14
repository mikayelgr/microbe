def _f(p):
    import hashlib, binascii
    s = hashlib.sha256()
    buf = bytearray(512)
    mv = memoryview(buf)
    with open(p, "rb") as fp:
        while True:
            n = fp.readinto(buf)
            if not n:
                break
            s.update(mv[:n])
    print(binascii.hexlify(s.digest()).decode())


_f({{path}})
del _f
