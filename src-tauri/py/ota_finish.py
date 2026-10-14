# Read back the first {{length}} bytes of the new image and print their SHA-256.
def _mb_ota_hash(n):
    import hashlib, binascii
    h = hashlib.sha256()
    buf = bytearray(4096)
    b = 0
    while n > 0:
        _mb_ota.readblocks(b, buf)
        h.update(buf if n >= 4096 else buf[:n])
        n -= 4096
        b += 1
    print(binascii.hexlify(h.digest()).decode())


_mb_ota_hash({{length}})
del _mb_ota_hash
