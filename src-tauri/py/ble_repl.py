# Installed by Microbe: exposes the REPL over the BLE Nordic UART Service.
# Import it from boot.py (`import ble_repl; ble_repl.start()`).
import bluetooth, io, os, micropython
from micropython import const

_IRQ_CENTRAL_CONNECT = const(1)
_IRQ_CENTRAL_DISCONNECT = const(2)
_IRQ_GATTS_WRITE = const(3)
_FLAG_WRITE = const(0x0008)
_FLAG_WRITE_NO_RESPONSE = const(0x0004)
_FLAG_NOTIFY = const(0x0010)
_UART = bluetooth.UUID("6E400001-B5A3-F393-E0A9-E50E24DCCA9E")
_TX = (bluetooth.UUID("6E400003-B5A3-F393-E0A9-E50E24DCCA9E"), _FLAG_NOTIFY)
_RX = (bluetooth.UUID("6E400002-B5A3-F393-E0A9-E50E24DCCA9E"), _FLAG_WRITE | _FLAG_WRITE_NO_RESPONSE)


def _adv(name):
    n = name.encode()
    p = bytearray(b"\x02\x01\x06") + bytes((len(n) + 1, 0x09)) + n
    u = bytes(_UART)
    return p + bytes((len(u) + 1, 0x07)) + u


class _Stream(io.IOBase):
    def __init__(self, name):
        self._ble = bluetooth.BLE()
        self._ble.active(True)
        self._ble.irq(self._irq)
        ((self._tx, self._rx),) = self._ble.gatts_register_services(((_UART, (_TX, _RX)),))
        self._ble.gatts_set_buffer(self._rx, 256, True)
        self._conns = set()
        self._in = bytearray()
        self._adv_data = _adv(name)
        self._advertise()

    def _advertise(self):
        self._ble.gap_advertise(100000, adv_data=self._adv_data)

    def _irq(self, event, data):
        if event == _IRQ_CENTRAL_CONNECT:
            self._conns.add(data[0])
        elif event == _IRQ_CENTRAL_DISCONNECT:
            self._conns.discard(data[0])
            self._advertise()
        elif event == _IRQ_GATTS_WRITE:
            self._in += self._ble.gatts_read(self._rx)
            if hasattr(os, "dupterm_notify"):
                os.dupterm_notify(None)

    def readinto(self, buf):
        n = min(len(buf), len(self._in))
        if not n:
            return None
        buf[:n] = self._in[:n]
        self._in = self._in[n:]
        return n

    def ioctl(self, op, arg):
        if op == 3:  # MP_STREAM_POLL
            return 1 if self._in else 0
        return 0

    def write(self, buf):
        mv = memoryview(buf)
        for c in self._conns:
            for i in range(0, len(mv), 20):
                try:
                    self._ble.gatts_notify(c, self._tx, mv[i:i + 20])
                except OSError:
                    pass
        return len(buf)


def start(name="mpy-repl"):
    os.dupterm(_Stream(name))
