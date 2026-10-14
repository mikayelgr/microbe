try:
    import asyncio
    _c = asyncio.core
    if hasattr(_c, "__mb_orig"):
        for _t in _c.__mb_spy:
            _t.cancel()
        _c.create_task = _c.__mb_orig
        asyncio.create_task = _c.__mb_orig
        del _c.__mb_orig, _c.__mb_spy
    del _c
except Exception:
    pass
