def _f(p):
    import os
    q = ""
    for i, part in enumerate(p.split("/")):
        q = part if i == 0 else q + "/" + part
        if part in ("", "."):
            continue
        try:
            os.mkdir(q)
        except OSError as e:
            if e.args[0] != 17:  # EEXIST
                raise


_f({{path}})
del _f
