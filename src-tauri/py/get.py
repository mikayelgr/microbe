def _f(p, n):
    import os, binascii
    print(os.stat(p)[6])
    with open(p, "rb") as f:
        while True:
            b = f.read(n)
            if not b:
                break
            print(binascii.b2a_base64(b).decode().strip())


_f({{path}}, {{chunk}})
del _f
