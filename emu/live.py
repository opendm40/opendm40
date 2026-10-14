#!/usr/bin/python3
"""Interactive DM40 emulator window.

    emu/live.py [stock|fwrs|chain] [--fast] [--app FILE] [--tk] [run.py options...]

A GTK4/libadwaita window (Tk fallback with --tk or when gi is missing) that
draws the DM40 front panel as on the device (manual figure 2.1.1): the
display in its glass with the MENU/RUN/AUTO/MF touch strip on its left, the
power button on the left edge, USB-C / LED / MCX generator / MCX scope on the
top edge, the V-Ohm / COM / A jacks on the right. The LCD is a memory-mapped
framebuffer written by the ILI9488 model at 30 fps, shown at an integer scale
(nearest neighbour). Everything else goes through Renode's monitor TCP port,
so the GUI never blocks the emulation.

Mouse     on the display: press/drag/release = touch (tap, drag, swipe);
          Ctrl+drag = two-finger pinch around the click point. On the key
          strip: press-and-hold = the key (held = long press). Power button
          = PD6. Jacks / MCX / USB open the matching input page in the sidebar.
Keyboard  m r a f = MENU RUN AUTO MF, p = power, F2/F3 = 2-/3-finger swipe
          down, space = pause/resume, s = screenshot, F9 = sidebar,
          Esc / Ctrl-C / window close = quit.
Needs /usr/bin/python3 (PyGObject + libadwaita, pycairo, numpy, PIL).
"""
import argparse
import mmap
import os
import queue
import signal
import socket
import struct
import subprocess
import sys
import threading
import time
import tkinter as tk
from tkinter import ttk

import numpy as np
from PIL import Image, ImageTk

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run  # noqa: E402  (emu/run.py: image composition, script generation)

W, H, HDR = 480, 320, 64
KEYS = {'m': 0, 'r': 1, 'a': 2, 'f': 3}
KEY_NAMES = ['MENU', 'RUN', 'AUTO', 'MF']


def free_port():
    s = socket.socket()
    s.bind(('127.0.0.1', 0))
    p = s.getsockname()[1]
    s.close()
    return p


class Emulator:
    """One Renode process: monitor socket + live framebuffer."""

    def __init__(self, mode, extra):
        self.mode = mode
        self.port = free_port()
        self.fb_path = os.path.join(run.OUT, 'live-fb.bin')
        self.log_path = os.path.join(run.OUT, 'live-renode.out')
        self.monitor_log = open(os.path.join(run.OUT, 'live-monitor.log'), 'w')
        if os.path.exists(self.fb_path):
            os.unlink(self.fb_path)
        argv = [mode, '--interactive', '--dry', '--run-for', '0', '--name', 'live',
                '--live', self.fb_path] + extra
        resc = run.main(argv)
        # No --console: it disables the -P monitor port (verified with 1.17.0).
        cmd = [run.RENODE, '--disable-xwt', '--hide-log', '-P', str(self.port),
               '-e', 'i @' + os.path.abspath(resc)]
        self.out = open(self.log_path, 'w')
        self.proc = subprocess.Popen(cmd, stdin=subprocess.PIPE, stdout=self.out,
                                     stderr=subprocess.STDOUT, start_new_session=True)
        self.q = queue.Queue()
        self.sock = None
        self.mm = None
        self.paused = False
        self.last_reply = ''
        threading.Thread(target=self._sender, daemon=True).start()

    # ---- monitor ----
    def send(self, cmd):
        self.q.put(cmd)

    def _connect(self):
        for _ in range(600):
            if self.proc.poll() is not None:
                return False
            try:
                self.sock = socket.create_connection(('127.0.0.1', self.port), timeout=1)
                self.sock.settimeout(None)
                threading.Thread(target=self._reader, daemon=True).start()
                return True
            except OSError:
                time.sleep(0.1)
        return False

    def _reader(self):
        buf = b''
        while True:
            try:
                d = self.sock.recv(4096)
            except OSError:
                return
            if not d:
                return
            try:
                self.monitor_log.write(bytes(c for c in d if 32 <= c < 127 or c == 10).decode(errors='replace'))
                self.monitor_log.flush()
            except (OSError, ValueError):
                pass
            buf = (buf + d)[-4096:]
            text = bytes(c for c in buf if 32 <= c < 127 or c == 10).decode(errors='replace')
            lines = [l for l in text.splitlines() if l.strip() and not l.startswith('(')]
            if lines:
                self.last_reply = lines[-1][-120:]

    def _sender(self):
        if not self._connect():
            return
        while True:
            cmd = self.q.get()
            if cmd is None:
                return
            try:
                self.monitor_log.write('> %s\n' % cmd)
                self.sock.sendall((cmd + '\n').encode())
            except (OSError, ValueError):
                return

    # ---- framebuffer ----
    def frame(self):
        """-> (seq, virtual_us, pixels, rgb array HxWx3) or None."""
        if self.mm is None:
            if not os.path.exists(self.fb_path) or os.path.getsize(self.fb_path) < HDR + W * H * 2:
                return None
            f = open(self.fb_path, 'rb')
            self.mm = mmap.mmap(f.fileno(), 0, access=mmap.ACCESS_READ)
            f.close()
        seq, = struct.unpack_from('<I', self.mm, 8)
        vus, pix, frames = struct.unpack_from('<QQQ', self.mm, 24)
        px = np.frombuffer(self.mm, dtype='<u2', count=W * H, offset=HDR).reshape(H, W)
        rgb = np.empty((H, W, 3), np.uint8)
        r = (px >> 11) & 0x1F
        g = (px >> 5) & 0x3F
        b = px & 0x1F
        rgb[..., 0] = (r << 3) | (r >> 2)
        rgb[..., 1] = (g << 2) | (g >> 4)
        rgb[..., 2] = (b << 3) | (b >> 2)
        return seq, vus, pix, rgb

    def alive(self):
        return self.proc.poll() is None

    def stop(self):
        self.q.put(None)
        try:
            if self.sock:
                self.sock.close()
        except OSError:
            pass
        if self.mm is not None:
            self.mm.close()
            self.mm = None
        run.kill_group(self.proc, 2.0)
        try:
            self.proc.stdin.close()
        except OSError:
            pass
        self.out.close()
        if os.path.exists(self.fb_path):
            os.unlink(self.fb_path)


class TkApp:
    def __init__(self, args, extra):
        self.args, self.extra = args, extra
        self.root = tk.Tk()
        self.root.title('DM40 emulator')
        self.scale = args.scale
        self.emu = None
        self.img = None
        self.pinch = None
        self.last_move = 0
        self.key_down = {}
        self.rate_hist = []

        top = ttk.Frame(self.root)
        top.pack(side='top', fill='x')
        self.mode = tk.StringVar(value=args.mode)
        ttk.Label(top, text='firmware').pack(side='left')
        cb = ttk.Combobox(top, textvariable=self.mode, values=['stock', 'fwrs', 'chain'], width=7, state='readonly')
        cb.pack(side='left')
        cb.bind('<<ComboboxSelected>>', lambda e: self.restart())
        self.fast = tk.BooleanVar(value=args.fast)
        ttk.Checkbutton(top, text='fast', variable=self.fast, command=self.restart).pack(side='left')
        ttk.Button(top, text='restart', command=self.restart).pack(side='left')
        self.pause_btn = ttk.Button(top, text='pause', command=self.toggle_pause)
        self.pause_btn.pack(side='left')
        ttk.Button(top, text='screenshot', command=self.screenshot).pack(side='left')

        self.canvas = tk.Canvas(self.root, width=W * self.scale, height=H * self.scale, bg='black',
                                highlightthickness=0)
        self.canvas.pack(side='top')
        self.canvas_img = self.canvas.create_image(0, 0, anchor='nw')
        self.canvas.bind('<ButtonPress-1>', self.on_press)
        self.canvas.bind('<B1-Motion>', self.on_motion)
        self.canvas.bind('<ButtonRelease-1>', self.on_release)

        keys = ttk.Frame(self.root)
        keys.pack(side='top', fill='x')
        for code, name in enumerate(KEY_NAMES):
            b = ttk.Button(keys, text=name)
            b.pack(side='left')
            b.bind('<ButtonPress-1>', lambda e, c=code: self.side_key(c, True))
            b.bind('<ButtonRelease-1>', lambda e, c=code: self.side_key(c, False))
        b = ttk.Button(keys, text='POWER')
        b.pack(side='left')
        b.bind('<ButtonPress-1>', lambda e: self.power(True))
        b.bind('<ButtonRelease-1>', lambda e: self.power(False))
        ttk.Button(keys, text='2-finger swipe down', command=lambda: self.multi_swipe(2)).pack(side='left')
        ttk.Button(keys, text='3-finger swipe down', command=lambda: self.multi_swipe(3)).pack(side='left')

        inp = ttk.Frame(self.root)
        inp.pack(side='top', fill='x')
        ttk.Label(inp, text='DMM V').pack(side='left')
        self.vin = tk.StringVar(value='0.0')
        e = ttk.Entry(inp, textvariable=self.vin, width=8)
        e.pack(side='left')
        e.bind('<Return>', lambda ev: self.set_vin())
        ttk.Button(inp, text='set', command=self.set_vin).pack(side='left')
        ttk.Label(inp, text='   scope').pack(side='left')
        self.wave = tk.StringVar(value='sine')
        ttk.Combobox(inp, textvariable=self.wave, values=['sine', 'square', 'triangle', 'ramp', 'dc', 'noise'],
                     width=8, state='readonly').pack(side='left')
        self.freq = tk.StringVar(value='1000')
        self.vpp = tk.StringVar(value='1.0')
        self.voff = tk.StringVar(value='0.0')
        for lbl, var in (('Hz', self.freq), ('Vpp', self.vpp), ('offset', self.voff)):
            ttk.Entry(inp, textvariable=var, width=8).pack(side='left')
            ttk.Label(inp, text=lbl).pack(side='left')
        ttk.Button(inp, text='set', command=self.set_wave).pack(side='left')

        self.status = tk.StringVar(value='starting...')
        ttk.Label(self.root, textvariable=self.status, anchor='w').pack(side='top', fill='x')

        for k in KEYS:
            self.root.bind('<KeyPress-%s>' % k, lambda e, c=KEYS[k]: self.side_key(c, True))
            self.root.bind('<KeyRelease-%s>' % k, lambda e, c=KEYS[k]: self.key_release(c))
        self.root.bind('<KeyPress-p>', lambda e: self.power(True))
        self.root.bind('<KeyRelease-p>', lambda e: self.power(False))
        self.root.bind('<F2>', lambda e: self.multi_swipe(2))
        self.root.bind('<F3>', lambda e: self.multi_swipe(3))
        self.root.bind('<space>', lambda e: self.toggle_pause())
        self.root.bind('<KeyPress-s>', lambda e: self.screenshot())
        self.root.bind('<Escape>', lambda e: self.quit())
        self.root.bind('<Control-c>', lambda e: self.quit())
        self.root.protocol('WM_DELETE_WINDOW', self.quit)
        signal.signal(signal.SIGINT, lambda *a: self.root.after(0, self.quit))
        signal.signal(signal.SIGTERM, lambda *a: self.root.after(0, self.quit))

        self.start()
        self.root.after(33, self.refresh)
        self.root.after(200, self.tick)  # lets Python see SIGINT while Tk waits

    # ---- emulator lifecycle ----
    def start(self):
        extra = list(self.extra)
        if self.args.app:
            extra += ['--app', self.args.app]
        if self.fast.get():
            extra += ['--fast']
        self.status.set('building / starting %s ...' % self.mode.get())
        self.root.update_idletasks()
        self.emu = Emulator(self.mode.get(), extra)
        self.t_start = time.time()
        self.rate_hist = []

    def restart(self):
        if self.emu:
            self.emu.stop()
        self.start()

    def quit(self):
        if self.emu:
            self.emu.stop()
            self.emu = None
        self.root.destroy()

    def tick(self):
        self.root.after(200, self.tick)

    # ---- display ----
    def refresh(self):
        self.root.after(33, self.refresh)
        if not self.emu:
            return
        if not self.emu.alive():
            self.status.set('Renode exited (see %s)' % run.rel(self.emu.log_path))
            return
        f = self.emu.frame()
        if f is None:
            self.status.set('waiting for the emulator ... %.0f s' % (time.time() - self.t_start))
            return
        seq, vus, pix, rgb = f
        im = Image.fromarray(rgb, 'RGB')
        self.last_image = im
        if self.scale != 1:
            im = im.resize((W * self.scale, H * self.scale), Image.NEAREST)
        self.img = ImageTk.PhotoImage(im)
        self.canvas.itemconfigure(self.canvas_img, image=self.img)
        now = time.time()
        self.rate_hist.append((now, vus))
        self.rate_hist = [p for p in self.rate_hist if now - p[0] < 2.0]
        ratio = 0.0
        if len(self.rate_hist) > 1 and now - self.rate_hist[0][0] > 0.2:
            ratio = (vus - self.rate_hist[0][1]) / 1e6 / (now - self.rate_hist[0][0])
        self.status.set('%s  virtual %.2f s  speed %.2fx real time  %s  %s' % (
            self.mode.get(), vus / 1e6, ratio, 'PAUSED' if self.emu.paused else '', self.emu.last_reply))

    def screenshot(self):
        if getattr(self, 'last_image', None) is None:
            return
        path = os.path.join(run.OUT, 'live-%s.png' % time.strftime('%Y%m%d-%H%M%S'))
        self.last_image.save(path)
        self.status.set('saved ' + run.rel(path))

    # ---- input ----
    def xy(self, e):
        return max(0, min(W - 1, e.x // self.scale)), max(0, min(H - 1, e.y // self.scale))

    def touch_cmd(self, *pts):
        self.emu.send('sysbus.i2c3.touch Touch "%s"' % ';'.join('%d,%d' % p for p in pts))

    def pinch_points(self, x):
        cx, cy, x0 = self.pinch
        d = max(10, 60 + (x - x0))
        return (max(0, cx - d), cy), (min(W - 1, cx + d), cy)

    def on_press(self, e):
        if not self.emu:
            return
        x, y = self.xy(e)
        if e.state & 0x4:  # Ctrl: pinch
            self.pinch = (x, y, x)
            self.touch_cmd(*self.pinch_points(x))
        else:
            self.pinch = None
            self.emu.send('sysbus.i2c3.touch Press %d %d' % (x, y))
        self.last_move = time.time()

    def on_motion(self, e):
        if not self.emu or time.time() - self.last_move < 0.02:
            return
        self.last_move = time.time()
        x, y = self.xy(e)
        if self.pinch:
            self.touch_cmd(*self.pinch_points(x))
        else:
            self.emu.send('sysbus.i2c3.touch Move %d %d' % (x, y))

    def on_release(self, e):
        if not self.emu:
            return
        if self.pinch:
            self.touch_cmd()
            self.pinch = None
        else:
            self.emu.send('sysbus.i2c3.touch Release')

    def side_key(self, code, down):
        if not self.emu:
            return
        if down:
            if self.key_down.get(code):
                self.key_down[code] = 'held'  # keyboard auto-repeat
                return
            self.key_down[code] = True
            self.emu.send('sysbus.i2c3.touch KeyDown %d' % code)
        else:
            if self.key_down.pop(code, None):
                self.emu.send('sysbus.i2c3.touch KeyUp')

    def key_release(self, code):
        # X11 auto-repeat sends release+press pairs: release only if no press follows.
        self.key_down[code] = True
        self.root.after(40, lambda: self._key_release_check(code))

    def _key_release_check(self, code):
        if self.key_down.get(code) is True:
            self.side_key(code, False)
        elif self.key_down.get(code) == 'held':
            self.key_down[code] = True

    def power(self, down):
        if self.emu:
            self.emu.send('sysbus.gpioPortD OnGPIO 6 %s' % ('true' if down else 'false'))

    def multi_swipe(self, n):
        if self.emu:
            self.emu.send('sysbus.i2c3.touch MultiSwipe %d 240 80 0 120 300' % n)

    def set_vin(self):
        if self.emu:
            # HY3131 model API (emu/README.md, HY3131 section).
            self.emu.send('sysbus.hy3131 SetVin %s' % float(self.vin.get()))

    def set_wave(self):
        if self.emu:
            self.emu.send('sysbus.scopefeed SetWave "%s" %g %g %g' % (
                self.wave.get(), float(self.freq.get()), float(self.vpp.get()), float(self.voff.get())))

    def toggle_pause(self):
        if not self.emu:
            return
        self.emu.paused = not self.emu.paused
        self.emu.send('pause' if self.emu.paused else 'start')
        self.pause_btn.configure(text='resume' if self.emu.paused else 'pause')


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument('mode', nargs='?', default='stock', choices=['stock', 'fwrs', 'chain'])
    ap.add_argument('--fast', action='store_true', help='PerformanceInMips 50 (about 2x faster)')
    ap.add_argument('--app', help='application image (run.py --app)')
    ap.add_argument('--scale', type=int, default=2, help='Tk window scale (default 2)')
    ap.add_argument('--tk', action='store_true', help='use the Tk window instead of GTK')
    args, extra = ap.parse_known_args()
    if not args.tk:
        try:
            import gi
            gi.require_version('Gtk', '4.0')
            gi.require_version('Adw', '1')
            gi.require_version('Gsk', '4.0')
            gi.require_version('Graphene', '1.0')
            import gtkpanel
        except (ImportError, ValueError) as e:
            print('GTK4/libadwaita not available (%s): using the Tk window' % e, file=sys.stderr)
            args.tk = True
    if not args.tk:
        sys.exit(gtkpanel.run_app(args, extra, Emulator))
    app = TkApp(args, extra)
    try:
        app.root.mainloop()
    finally:
        if app.emu:
            app.emu.stop()


if __name__ == '__main__':
    main()
