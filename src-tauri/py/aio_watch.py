# Prelude: report asyncio tasks every {{period_ms}} ms as
# `\x1eTASKS {"now": ms, "tasks": [...]}` records. Tasks are tracked by
# wrapping asyncio's create_task; aio_watch_end restores it.
def __mb_aio_watch(period):
    try:
        import asyncio, time
        core = asyncio.core
    except (ImportError, AttributeError):
        return
    if hasattr(core, "__mb_orig"):
        return
    orig = core.create_task
    tasks = []
    spy = []

    def name_of(t):
        r = repr(t.coro)
        i = r.find("'")
        j = r.find("'", i + 1)
        return r[i + 1 : j] if 0 <= i < j else r

    def jstr(s):
        return '"' + str(s).replace("\\", "\\\\").replace('"', '\\"').replace("\n", " ") + '"'

    def snapshot():
        now = time.ticks_ms()
        cur = core.cur_task
        out = []
        keep = []
        for t in tasks:
            on = "null"
            wake = "null"
            err = "null"
            if t is cur:
                st = "running"
            elif t.done():
                d = t.data
                if d is None or isinstance(d, StopIteration):
                    st = "done"
                elif isinstance(d, core.CancelledError):
                    st = "cancelled"
                else:
                    st = "failed"
                    err = jstr("%s: %s" % (type(d).__name__, d))
            elif t.data is not None:
                st = "waiting"
                on = jstr(repr(t.data).strip("<>"))
            else:
                left = time.ticks_diff(t.ph_key, now)
                st = "sleeping" if left > 0 else "ready"
                wake = str(max(left, 0))
            out.append('{"id": %d, "name": %s, "state": "%s", "wake_ms": %s, "on": %s, "error": %s}' % (id(t), jstr(name_of(t)), st, wake, on, err))
            if not t.done():
                keep.append(t)
        tasks[:] = keep
        print('\x1eTASKS {"now": %d, "tasks": [%s]}' % (now, ", ".join(out)))

    async def watch():
        while True:
            snapshot()
            await core.sleep_ms(period)

    def create_task(coro):
        t = orig(coro)
        tasks.append(t)
        if not spy:
            spy.append(orig(watch()))
        return t

    core.__mb_orig = orig
    core.__mb_spy = spy
    core.create_task = create_task
    asyncio.create_task = create_task


__mb_aio_watch({{period_ms}})
del __mb_aio_watch
