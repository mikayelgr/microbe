# Prelude: sample heap use every {{period_ms}} ms while the program runs, as
# `\x1eMEM {"free": n, "alloc": n}` records. Needs a machine.Timer; ports
# without one (the unix port) just don't report. Stopped by mem_watch_end.
def __mb_mem_watch(period):
    import gc
    try:
        from machine import Timer
        import micropython
    except ImportError:
        return

    def emit(_):
        print("\x1eMEM {\"free\": %d, \"alloc\": %d}" % (gc.mem_free(), gc.mem_alloc()))

    def tick(_):
        try:
            micropython.schedule(emit, None)
        except RuntimeError:
            pass  # schedule queue full; skip this sample

    emit(None)
    for tid in (-1, 0):
        try:
            t = Timer(tid)
            t.init(period=period, mode=Timer.PERIODIC, callback=tick)
            globals()["__mb_mem_timer"] = t
            return
        except Exception:
            pass


__mb_mem_watch({{period_ms}})
del __mb_mem_watch
