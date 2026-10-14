# Describe modules for stub generation: one JSON line per module (and per
# submodule), `{"module", "members": [[name, kind, type]], "classes": {...}}`.
def _mb_introspect(names):
    import json

    plain = ("int", "float", "str", "bool", "bytes", "bytearray", "tuple", "list", "dict", "NoneType")
    funcs = ("function", "builtin_function", "bound_method", "closure", "staticmethod", "classmethod", "generator")

    def kind(v):
        t = type(v)
        n = t.__name__
        if n in funcs:
            return "function"
        if t is type:
            return "class"
        if n == "module":
            return "module"
        if n in plain:
            return n
        return "object"

    def members(obj):
        out = []
        for a in dir(obj):
            if a.startswith("__") and a != "__init__":
                continue
            try:
                v = getattr(obj, a)
            except Exception:
                continue
            out.append((a, kind(v), type(v).__name__, v))
        return out

    def top_level(name):
        try:
            __import__(name)
            return True
        except Exception:
            return False

    def emit(name, mod, depth):
        flat, classes, subs = [], {}, []
        for a, k, tn, v in members(mod):
            if a == "__init__":
                continue
            flat.append([a, k, tn])
            if k == "class":
                classes[a] = [[x, kk, tt] for x, kk, tt, _ in members(v)]
            elif k == "module" and depth < 2 and not top_level(getattr(v, "__name__", a)):
                subs.append((a, v))
        print(json.dumps({"module": name, "members": flat, "classes": classes}))
        for a, v in subs:
            emit(name + "." + a, v, depth + 1)

    for n in json.loads(names):
        try:
            m = __import__(n)
            for part in n.split(".")[1:]:
                m = getattr(m, part)
        except BaseException as e:
            print(json.dumps({"module": n, "error": repr(e)}))
            continue
        emit(n, m, 0)


_mb_introspect({{modules}})
del _mb_introspect
