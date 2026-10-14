# Prelude for the debugger (see src-tauri/src/debug.rs). The program that
# follows is instrumented with calls to _mbdbg.l / .e / .s. While stopped, the
# debugger reads one-line commands from stdin.
class _MbDbg:
    def __init__(self, bps, fns):
        import json
        self.json = json
        self.bps = set(json.loads(bps))
        self.fns = json.loads(fns)
        self.stack = [0]
        self.mode = "run"
        self.depth = 0
        self.line = 0
        self.why = ""
        # Globals of these types aren't shown as variables.
        self.hidden = (type, type(print), type(lambda: 0), type(self.read))
        self.pause = False
        # Commands sent while running (pause, breakpoint changes) are picked
        # up by polling stdin at each hook, where the port supports it.
        try:
            import select, sys
            self.poller = select.poll()
            self.poller.register(sys.stdin, select.POLLIN)
        except Exception:
            self.poller = None

    def e(self, f):
        self.stack.append(f)

    def l(self, line, f):
        if self.poller and self.poller.poll(0):
            self.command(self.read())
        st = self.stack
        if f not in st:
            st.append(f)
        elif st[-1] != f:
            while st[-1] != f:
                st.pop()
        d = len(st)
        m = self.mode
        if self.pause:
            self.pause = False
            self.why = "pause"
        elif line in self.bps:
            self.why = "breakpoint"
        elif m == "step" or (m == "next" and d <= self.depth) or (m == "out" and d < self.depth):
            self.why = "step"
        else:
            return False
        self.line = line
        return True

    def r(self, v):
        try:
            s = repr(v)
        except Exception as ex:
            s = "<repr failed: %s>" % type(ex).__name__
        return s if len(s) <= 300 else s[:297] + "..."

    def emit(self, d):
        print("\x1eDBG", self.json.dumps(d))

    def read(self):
        import sys
        buf = ""
        while True:
            c = sys.stdin.read(1)
            if c in ("\r", "\n"):
                if buf:
                    return buf
            elif c:
                buf += c

    def command(self, cmd):
        """Commands valid both while running and while stopped."""
        if cmd == "p":
            self.pause = True
        elif cmd.startswith("b"):
            self.bps = set(int(x) for x in cmd[1:].strip().split(",") if x)
        else:
            return False
        return True

    def s(self, pairs):
        g = globals()
        loc = {}
        for n, get in pairs:
            try:
                loc[n] = self.r(get())
            except Exception:
                pass
        glb = {}
        for n in g:
            if n.startswith("_"):
                continue
            v = g[n]
            t = type(v)
            if t in self.hidden or t.__name__ in ("module", "generator", "closure"):
                continue
            glb[n] = self.r(v)
        self.emit({"event": "stopped", "line": self.line, "reason": self.why,
                   "stack": [self.fns[i] for i in self.stack], "locals": loc, "globals": glb})
        while True:
            cmd = self.read()
            if cmd == "c":
                self.mode = "run"
                return
            if cmd in ("s", "n", "o"):
                self.mode = {"s": "step", "n": "next", "o": "out"}[cmd]
                self.depth = len(self.stack)
                return
            if not self.command(cmd) and cmd.startswith("e "):
                try:
                    res, err = self.r(eval(cmd[2:], g)), None
                except Exception as ex:
                    res, err = None, "%s: %s" % (type(ex).__name__, ex)
                self.emit({"event": "eval", "expr": cmd[2:], "result": res, "error": err})


_mbdbg = _MbDbg({{breakpoints}}, {{functions}})
del _MbDbg
