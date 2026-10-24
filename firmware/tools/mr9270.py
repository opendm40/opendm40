#!/usr/bin/python3
"""Host-side driver for the MR9270S+ process calibrator (Modbus RTU).

Reusable class to source a known signal (voltage, mV, current, resistance,
frequency, PWM, ...) and read it back, used by verify_dmm.py to check the
Alientek DM40 running opendm40 against a reference.

Hardware / protocol (register map below; resistance: func 3 mode 1, i32 = 0.1 ohm, max 4000):
  - USB CDC-ACM, typically /dev/ttyACM1 (VID:PID 0483:5740).
  - Modbus RTU, 9600 8N1, slave id 1. Function 03 read / 16 write.
  - Holding-register protocol address = (40001-based register) - 40001.
  - Output value register is a 32-bit SIGNED int, BIG-ENDIAN WORD ORDER:
    high word at 0x10, low word at 0x11.

Value scaling (natural units -> i32 register value): see SCALE below. The
voltage scale (mV) is confirmed live; resistance / frequency / PWM scales were
determined read-before-write (verify_dmm.py --probe) and are recorded here.

Needs /usr/bin/python3 (pymodbus 3.8.6 lives there on this host).
"""
import sys

from pymodbus.client import ModbusSerialClient

# The highest DC voltage the unit really puts out: a "30 V" request gave
# 23.99 V on the DM40 (acdcmeth.py e1 / e5 / e6, 2026-10-23), and the value
# register reads back the setpoint, not the output, so a tool cannot see it.
# set_value caps "voltage" requests here (warning on stderr).
VOLTAGE_MAX = 24.0

# --- Holding-register map (protocol addresses) -----------------------------
REG_OUT_FUNC = 0x08     # output function / type, u16
REG_OUT_MODE = 0x09     # output signal mode, u16
REG_OUT_ENABLE = 0x0B   # output enable, u16 (1 = on)
REG_OUT_VALUE = 0x10    # output value i32, 0x10 high word, 0x11 low word
REG_MEAS_FUNC = 0x28    # measurement function, u16
REG_MEAS_MODE = 0x29    # measurement mode, u16
REG_MEAS_VALUE = 0x30   # measurement value i32, 0x30 high, 0x31 low

# --- Function table: name -> (func_code, mode_code, scale, unit) ------------
# i32_register_value = round(value_in_natural_units * scale)
# scale semantics confirmed live where noted; see the doc for evidence.
SCALE = {
    # name            func mode   scale     unit
    "voltage":        (1,  0,  1000.0,  "V"),    # i32 = millivolts  (CONFIRMED)
    "millivolt":      (1,  2,  1000.0,  "mV"),   # i32 = microvolts  (assumed, untested)
    "current_passive":(0,  0,  1000.0,  "mA"),   # i32 = microamps   (assumed, untested)
    "current_active": (0,  1,  1000.0,  "mA"),   # i32 = microamps   (assumed, untested)
    "resistance":     (3,  1,    10.0,  "ohm"),  # i32 = deci-ohms, max 4000 (CONFIRMED 2026-10-13 vs DM40)
    "frequency":      (2,  0,  1000.0,  "Hz"),   # i32 = millihertz  (CONFIRMED 2026-10-22: 40000 = 40.0017 Hz on
                                                 # the DM40 FREQ; 50 = a 0.05 Hz 0 / 9.52 V square, ~10 s low:
                                                 # the "1.0 = hertz" scale gave 50 Hz requests that, device-check doc)
    "pwm":            (2,  5,   100.0,  "%"),    # i32 = centi-percent (CONFIRMED, see doc)
    "speed_rps":      (2,  2,     1.0,  "rps"),  # untested
    "pwm_pct":        (2,  5,   100.0,  "%"),    # alias
}


def _to_i32(hi, lo):
    v = ((hi & 0xFFFF) << 16) | (lo & 0xFFFF)
    return v - (1 << 32) if v & 0x80000000 else v


def _from_i32(v):
    v &= 0xFFFFFFFF
    return [(v >> 16) & 0xFFFF, v & 0xFFFF]


class MR9270:
    """Minimal MR9270S+ calibrator driver over Modbus RTU."""

    def __init__(self, port="/dev/ttyACM1", slave=1, timeout=1.0):
        self.port = port
        self.slave = slave
        self._c = ModbusSerialClient(
            port=port, baudrate=9600, bytesize=8, parity="N",
            stopbits=1, timeout=timeout)
        self._cur = None  # current function name

    # -- connection ---------------------------------------------------------
    def connect(self):
        if not self._c.connect():
            raise IOError("cannot open calibrator at %s" % self.port)
        return self

    def close(self):
        self._c.close()

    def __enter__(self):
        return self.connect()

    def __exit__(self, *a):
        self.close()

    # -- low-level register access (slave= / device_id= compat) -------------
    def read(self, addr, n):
        try:
            r = self._c.read_holding_registers(addr, count=n, slave=self.slave)
        except TypeError:
            r = self._c.read_holding_registers(addr, count=n, device_id=self.slave)
        if r.isError():
            raise IOError("read 0x%02x x%d failed: %r" % (addr, n, r))
        return list(r.registers)

    def write(self, addr, values):
        if isinstance(values, int):
            values = [values]
        try:
            r = self._c.write_registers(addr, values, slave=self.slave)
        except TypeError:
            r = self._c.write_registers(addr, values, device_id=self.slave)
        if r.isError():
            raise IOError("write 0x%02x=%r failed: %r" % (addr, values, r))
        return r

    # -- diagnostics --------------------------------------------------------
    def dump(self):
        """Return the output-channel block 0x08..0x13 as a dict of hex strs."""
        regs = self.read(REG_OUT_FUNC, 12)  # 0x08..0x13
        names = {0: "0x08 func", 1: "0x09 mode", 3: "0x0B enable",
                 4: "0x0C rangemax?", 5: "0x0D", 6: "0x0E", 7: "0x0F",
                 8: "0x10 val_hi", 9: "0x11 val_lo", 10: "0x12", 11: "0x13"}
        d = {}
        for i, v in enumerate(regs):
            d[names.get(i, "0x%02x" % (REG_OUT_FUNC + i))] = "0x%04x" % v
        d["_value_i32"] = _to_i32(regs[8], regs[9])
        return d

    # -- output control -----------------------------------------------------
    def set_function(self, name):
        """Select the output function (disables output first)."""
        func, mode, _scale, _unit = SCALE[name]
        self.write(REG_OUT_ENABLE, 0)       # disable before switching
        self.write(REG_OUT_FUNC, func)
        self.write(REG_OUT_MODE, mode)
        self._cur = name

    def set_value(self, value):
        """Write the output value in natural units for the current function.

        Returns the i32 register value written, and the i32 read back."""
        if self._cur is None:
            raise RuntimeError("call set_function() first")
        scale = SCALE[self._cur][2]
        if self._cur == "voltage" and abs(value) > VOLTAGE_MAX:
            capped = VOLTAGE_MAX if value > 0 else -VOLTAGE_MAX
            print("mr9270: %g V requested, the calibrator gives at most %g V: set %g V"
                  % (value, VOLTAGE_MAX, capped), file=sys.stderr)
            value = capped
        iv = int(round(value * scale))
        self.write(REG_OUT_VALUE, _from_i32(iv))
        hi, lo = self.read(REG_OUT_VALUE, 2)
        return iv, _to_i32(hi, lo)

    def enable(self, on=True):
        self.write(REG_OUT_ENABLE, 1 if on else 0)

    def disable(self):
        self.enable(False)

    def set_output(self, name, value, enable=True):
        """Full sequence: select function, write value, then enable.

        Order matters: we disable, switch function, write the value, and only
        then enable, so the output never carries a stale value in new units.
        Returns (i32_written, i32_readback)."""
        self.set_function(name)
        written, readback = self.set_value(value)
        if enable:
            self.enable(True)
        return written, readback

    def read_measurement(self):
        """Read the calibrator's OWN measurement channel value (i32, raw)."""
        hi, lo = self.read(REG_MEAS_VALUE, 2)
        return _to_i32(hi, lo)

    def safe_shutdown(self):
        """Restore a low, safe output: 1 V voltage, enabled (the state the
        device was found in). Low-voltage and harmless."""
        self.set_output("voltage", 1.0, enable=True)


if __name__ == "__main__":
    # quick self-check: connect and dump the current output state
    with MR9270() as cal:
        print("output channel:", cal.dump())
        print("measurement i32:", cal.read_measurement())
