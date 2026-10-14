# Prelude for the test runner: defines __mb_tests. The test file's code runs
# next, then `__mb_tests.run(globals(), names)` with the test names the IDE
# found ("test_x" or "Class.test_x", in source order). Each result is a
# `\x1eTEST {...}` record; `\x1eTESTS {...}` closes the run.
class __MbTests:
    def emit(self, tag, d):
        import json
        print("\x1e" + tag, json.dumps(d))

    def call(self, fn, *a):
        r = fn(*a)
        if r is not None and hasattr(r, "send"):
            import asyncio
            asyncio.run(r)

    def run(self, g, names):
        import sys, time
        try:
            import io
        except ImportError:
            io = None
        counts = {"passed": 0, "failed": 0, "errors": 0, "skipped": 0}
        for name in names:
            cls_name, _, meth = name.rpartition(".")
            t0 = time.ticks_ms()
            status, message, tb = "pass", None, None
            try:
                if cls_name:
                    cls = g[cls_name]
                    try:
                        inst = cls()
                    except TypeError:
                        inst = cls(meth)
                    if hasattr(inst, "setUp"):
                        inst.setUp()
                    try:
                        self.call(getattr(inst, meth))
                    finally:
                        if hasattr(inst, "tearDown"):
                            inst.tearDown()
                else:
                    self.call(g[name])
            except BaseException as e:
                kind = type(e).__name__
                if kind in ("SkipTest", "Skip"):
                    status = "skip"
                elif isinstance(e, KeyboardInterrupt):
                    raise
                else:
                    status = "fail" if isinstance(e, AssertionError) else "error"
                    if io:
                        buf = io.StringIO()
                        sys.print_exception(e, buf)
                        tb = buf.getvalue()
                message = str(e) or kind
            key = {"pass": "passed", "fail": "failed", "error": "errors", "skip": "skipped"}[status]
            counts[key] += 1
            self.emit("TEST", {"name": name, "status": status, "ms": time.ticks_diff(time.ticks_ms(), t0), "message": message, "traceback": tb})
        self.emit("TESTS", counts)


__mb_tests = __MbTests()
del __MbTests
# Keep `if __name__ == "__main__": unittest.main()` in test files from running
# everything a second time. testrunner_end restores it.
__name__ = "__mb_test__"
