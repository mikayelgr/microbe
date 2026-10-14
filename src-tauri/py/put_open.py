_mb_f = open({{path}}, "wb")
_mb_w = _mb_f.write
try:
    from binascii import a2b_base64 as _mb_a
except ImportError:
    _mb_a = None
print(1 if _mb_a else 0)
