"""GTK4/libadwaita window for emu/live.py: the DM40 front panel.

Geometry is in millimetres of the real device (140 x 83 mm, manual figure
2.1.1): a 480x320 display of 73.5 x 49 mm in a black glass with the
MENU/RUN/AUTO/MF touch strip on its left, power button on the left edge,
USB-C / charge LED / MCX generator / MCX scope on the top edge, V-Ohm / COM /
A banana jacks on the right. The display is rendered at an integer device-pixel
scale k (nearest neighbour), the rest with Cairo at 480k/73.5 px per mm.
"""
import os
import signal
import sys
import threading
import time

import gi

gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
gi.require_version('Gsk', '4.0')
gi.require_version('Graphene', '1.0')
import cairo  # noqa: E402
from gi.repository import Adw, Gdk, GLib, Graphene, Gsk, Gtk  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run  # noqa: E402

W, H = 480, 320
DEV_W, DEV_H = 140.0, 83.0            # device body, mm
DISP = (22.5, 15.5, 73.5, 49.0)       # display x, y, w, h (mm)
GLASS = (11.0, 8.0, 87.0, 64.0)       # black glass (keys + logo + display)
STRIP = (12.0, 15.5, 9.0, 49.0)       # touch key strip
POWER = (0.0, 30.0, 6.5, 14.0)        # power button on the left edge
TOP = {'usb': (22.0, 0.0, 9.0, 6.5), 'led': (34.0, 1.5, 3.0, 3.0), 'gen': (37.5, 0.0, 7.0, 7.0), 'scope': (58.0, 0.0, 7.0, 7.0)}
JACKS = {'v': (124.0, 20.0), 'com': (124.0, 41.0), 'a': (124.0, 62.0)}
JACK_R = 4.2
KEY_NAMES = ['MENU', 'RUN', 'AUTO', 'MF']
KEY_CHARS = {'m': 0, 'r': 1, 'a': 2, 'f': 3}
PX_PER_MM_1X = W / DISP[2]           # 6.53 px/mm at k = 1


def rgb(h):
    return tuple(int(h[i:i + 2], 16) / 255 for i in (1, 3, 5))


C_HOLSTER = rgb('#F3B51E')
C_HOLSTER_DARK = rgb('#C78F0A')
C_FACE = rgb('#2B2C2E')
C_GLASS = rgb('#0B0C0E')
C_STRIP_TEXT = rgb('#E8E8E8')
C_PRESSED = rgb('#3584E4')
C_RED = rgb('#C8102E')
C_BLACK = rgb('#111111')
C_LABEL = rgb('#D8D8D8')


class FrontPanel(Gtk.Widget):
    """Draws the panel, shows the framebuffer texture, hit-tests the controls."""

    def __init__(self, on_event):
        super().__init__()
        self.on_event = on_event      # callback(kind, data)
        self.texture = None
        self.pressed_key = None
        self.pressed_power = False
        self.backlight = 1.0
        self.drag_kind = None
        self.pinch = None
        self.last_move = 0.0
        self.set_hexpand(True)
        self.set_vexpand(True)
        drag = Gtk.GestureDrag()
        drag.connect('drag-begin', self.on_begin)
        drag.connect('drag-update', self.on_update)
        drag.connect('drag-end', self.on_end)
        self.add_controller(drag)
        motion = Gtk.EventControllerMotion()
        motion.connect('motion', self.on_hover)
        self.add_controller(motion)
        self.set_cursor_from_name('default')

    # ---- geometry ----
    def layout(self):
        """-> (k, s, ox, oy): display scale k (device px per LCD px), s px/mm,
        panel origin (logical px)."""
        sf = self.get_scale_factor()
        aw, ah = self.get_width(), self.get_height()
        k = max(1, int(min(aw * sf / (DEV_W * PX_PER_MM_1X), ah * sf / (DEV_H * PX_PER_MM_1X))))
        s = PX_PER_MM_1X * k / sf
        ox = (aw - DEV_W * s) / 2
        oy = (ah - DEV_H * s) / 2
        return k, s, ox, oy

    def do_measure(self, orientation, for_size):
        if orientation == Gtk.Orientation.HORIZONTAL:
            n = int(DEV_W * PX_PER_MM_1X) + 1
        else:
            n = int(DEV_H * PX_PER_MM_1X) + 1
        return n, n, -1, -1

    def to_mm(self, x, y):
        _, s, ox, oy = self.layout()
        return (x - ox) / s, (y - oy) / s

    @staticmethod
    def inside(r, x, y):
        return r[0] <= x < r[0] + r[2] and r[1] <= y < r[1] + r[3]

    def hit(self, x, y):
        """-> ('display', (px, py)) | ('key', code) | ('power', None) | ('jack', name) | None"""
        mx, my = self.to_mm(x, y)
        if self.inside(DISP, mx, my):
            px = int((mx - DISP[0]) / DISP[2] * W)
            py = int((my - DISP[1]) / DISP[3] * H)
            return 'display', (max(0, min(W - 1, px)), max(0, min(H - 1, py)))
        if self.inside(STRIP, mx, my):
            return 'key', min(3, int((my - STRIP[1]) / (STRIP[3] / 4)))
        if self.inside(POWER, mx, my):
            return 'power', None
        for name, (cx, cy) in JACKS.items():
            if (mx - cx) ** 2 + (my - cy) ** 2 <= (JACK_R + 1.5) ** 2:
                return 'jack', name
        for name, r in TOP.items():
            if self.inside((r[0] - 1, r[1], r[2] + 2, r[3] + 3), mx, my):
                return 'jack', name
        return None

    # ---- input ----
    def on_hover(self, ctrl, x, y):
        h = self.hit(x, y)
        self.set_cursor_from_name('pointer' if h and h[0] != 'display' else 'default')

    def on_begin(self, g, x, y):
        self.drag_start = (x, y)
        h = self.hit(x, y)
        self.drag_kind = h[0] if h else None
        if not h:
            return
        state = g.get_current_event_state()
        ctrl = bool(state & Gdk.ModifierType.CONTROL_MASK)
        if h[0] == 'display':
            if ctrl:
                self.pinch = (h[1][0], h[1][1], x)
                self.on_event('touch', self.pinch_points(x))
            else:
                self.pinch = None
                self.on_event('press', h[1])
        elif h[0] == 'key':
            self.pressed_key = h[1]
            self.on_event('keydown', h[1])
            self.queue_draw()
        elif h[0] == 'power':
            self.pressed_power = True
            self.on_event('power', True)
            self.queue_draw()
        elif h[0] == 'jack':
            self.on_event('jack', h[1])

    def pinch_points(self, x):
        cx, cy, x0 = self.pinch
        _, s, _, _ = self.layout()
        d = max(10, 60 + int((x - x0) / s * PX_PER_MM_1X))
        return [(max(0, cx - d), cy), (min(W - 1, cx + d), cy)]

    def on_update(self, g, dx, dy):
        if self.drag_kind != 'display' or time.time() - self.last_move < 0.02:
            return
        self.last_move = time.time()
        x, y = self.drag_start[0] + dx, self.drag_start[1] + dy
        if self.pinch:
            self.on_event('touch', self.pinch_points(x))
        else:
            h = self.hit(x, y)
            if h and h[0] == 'display':
                self.on_event('move', h[1])
            else:
                mx, my = self.to_mm(x, y)
                px = max(0, min(W - 1, int((mx - DISP[0]) / DISP[2] * W)))
                py = max(0, min(H - 1, int((my - DISP[1]) / DISP[3] * H)))
                self.on_event('move', (px, py))

    def on_end(self, g, dx, dy):
        if self.drag_kind == 'display':
            if self.pinch:
                self.on_event('touch', [])
                self.pinch = None
            else:
                self.on_event('release', None)
        elif self.drag_kind == 'key':
            self.pressed_key = None
            self.on_event('keyup', None)
            self.queue_draw()
        elif self.drag_kind == 'power':
            self.pressed_power = False
            self.on_event('power', False)
            self.queue_draw()
        self.drag_kind = None

    # ---- drawing ----
    def do_snapshot(self, snapshot):
        k, s, ox, oy = self.layout()
        aw, ah = self.get_width(), self.get_height()
        cr = snapshot.append_cairo(Graphene.Rect().init(0, 0, aw, ah))
        cr.translate(ox, oy)
        cr.scale(s, s)
        self.draw_panel(cr)
        # Display: texture at an exact integer device-pixel scale.
        dx = ox + DISP[0] * s
        dy = oy + DISP[1] * s
        dw, dh = DISP[2] * s, DISP[3] * s
        if self.texture is not None:
            snapshot.append_scaled_texture(self.texture, Gsk.ScalingFilter.NEAREST,
                                           Graphene.Rect().init(dx, dy, dw, dh))
            if self.backlight < 0.98:
                snapshot.append_color(Gdk.RGBA(0, 0, 0, 1 - self.backlight), Graphene.Rect().init(dx, dy, dw, dh))
        else:
            snapshot.append_color(Gdk.RGBA(0.04, 0.04, 0.05, 1), Graphene.Rect().init(dx, dy, dw, dh))

    @staticmethod
    def rrect(cr, x, y, w, h, r):
        cr.new_sub_path()
        cr.arc(x + w - r, y + r, r, -1.5708, 0)
        cr.arc(x + w - r, y + h - r, r, 0, 1.5708)
        cr.arc(x + r, y + h - r, r, 1.5708, 3.14159)
        cr.arc(x + r, y + r, r, 3.14159, 4.71239)
        cr.close_path()

    def text(self, cr, x, y, txt, size, color=C_LABEL, align='left', bold=False, face='Cantarell'):
        cr.save()
        cr.select_font_face(face, cairo.FONT_SLANT_NORMAL,
                            cairo.FONT_WEIGHT_BOLD if bold else cairo.FONT_WEIGHT_NORMAL)
        cr.set_font_size(size)
        ext = cr.text_extents(txt)
        if align == 'center':
            x -= ext.x_advance / 2
        elif align == 'right':
            x -= ext.x_advance
        cr.move_to(x, y)
        cr.set_source_rgb(*color)
        cr.show_text(txt)
        cr.restore()

    def draw_panel(self, cr):
        # Holster (yellow bumper) and dark face
        cr.set_source_rgb(*C_HOLSTER)
        self.rrect(cr, 0, 0, DEV_W, DEV_H, 9)
        cr.fill()
        cr.set_source_rgb(*C_HOLSTER_DARK)
        cr.set_line_width(0.6)
        self.rrect(cr, 0.3, 0.3, DEV_W - 0.6, DEV_H - 0.6, 8.8)
        cr.stroke()
        cr.set_source_rgb(*C_FACE)
        self.rrect(cr, 4.5, 6.5, DEV_W - 9, DEV_H - 11, 5)
        cr.fill()
        # Glass
        cr.set_source_rgb(*C_GLASS)
        self.rrect(cr, *GLASS, 2.5)
        cr.fill()
        self.text(cr, GLASS[0] + 2.5, 13.2, 'ALIENTEK', 3.0, C_LABEL, bold=True)
        self.text(cr, GLASS[0] + 17.5, 13.3, '正点原子', 3.2, C_LABEL, bold=True, face='Noto Sans CJK SC')
        self.text(cr, GLASS[0] + GLASS[2] - 2.0, 13.2, 'DM40C', 3.0, C_LABEL, align='right', bold=True)
        # Key strip
        cell = STRIP[3] / 4
        for i, name in enumerate(KEY_NAMES):
            x, y = STRIP[0], STRIP[1] + i * cell
            if self.pressed_key == i:
                cr.set_source_rgba(*C_PRESSED, 0.55)
                self.rrect(cr, x + 0.4, y + 0.6, STRIP[2] - 0.8, cell - 1.2, 1.5)
                cr.fill()
            self.draw_key_icon(cr, i, x + STRIP[2] / 2, y + cell * 0.40)
            self.text(cr, x + STRIP[2] / 2, y + cell * 0.80, name, 2.1, C_STRIP_TEXT, align='center', bold=True)
        # Power button (left edge)
        px, py, pw, ph = POWER
        cr.set_source_rgb(*(C_PRESSED if self.pressed_power else C_BLACK))
        self.rrect(cr, px, py, pw + 1.5, ph, 2.5)
        cr.fill()
        cr.set_source_rgb(*C_LABEL)
        cr.set_line_width(0.5)
        cr.arc(px + pw / 2 + 0.5, py + ph / 2, 2.0, -1.1, 4.25)
        cr.stroke()
        cr.move_to(px + pw / 2 + 0.5, py + ph / 2 - 2.6)
        cr.line_to(px + pw / 2 + 0.5, py + ph / 2 - 0.4)
        cr.stroke()
        # Top edge connectors
        x, y, w, h = TOP['usb']
        cr.set_source_rgb(*C_BLACK)
        self.rrect(cr, x, y, w, h, 2.5)
        cr.fill()
        self.text(cr, x + w / 2, y + h + 3.0, 'USB-C', 1.9, C_LABEL, align='center')
        x, y, w, h = TOP['led']
        cr.set_source_rgb(0.25, 0.6, 1.0)
        cr.arc(x + w / 2, y + h / 2, w / 2, 0, 6.2832)
        cr.fill()
        for name, label in (('gen', 'GEN'), ('scope', 'SCOPE')):
            x, y, w, h = TOP[name]
            cr.set_source_rgb(*C_BLACK)
            cr.arc(x + w / 2, y + h / 2, w / 2, 0, 6.2832)
            cr.fill()
            cr.set_source_rgb(0.75, 0.75, 0.78)
            cr.arc(x + w / 2, y + h / 2, w / 2 - 1.2, 0, 6.2832)
            cr.fill()
            cr.set_source_rgb(*C_BLACK)
            cr.arc(x + w / 2, y + h / 2, 0.9, 0, 6.2832)
            cr.fill()
            self.text(cr, x + w / 2, y + h + 3.0, label, 1.9, C_LABEL, align='center')
        # Jacks
        for name, (cx, cy) in JACKS.items():
            cr.set_source_rgb(*(C_BLACK if name == 'com' else C_RED))
            cr.arc(cx, cy, JACK_R, 0, 6.2832)
            cr.fill()
            cr.set_source_rgb(0.08, 0.08, 0.08)
            cr.arc(cx, cy, JACK_R * 0.55, 0, 6.2832)
            cr.fill()
            cr.set_source_rgb(0.35, 0.35, 0.37)
            cr.set_line_width(0.4)
            cr.arc(cx, cy, JACK_R * 0.55, 0, 6.2832)
            cr.stroke()
        self.text(cr, 124.0, 27.8, 'V Ω ⊣⊢ Hz °C', 2.2, C_LABEL, align='center', bold=True, face='DejaVu Sans')
        self.text(cr, 124.0, 48.8, 'COM', 2.4, C_LABEL, align='center', bold=True)
        self.text(cr, 124.0, 69.8, 'A mA uA', 2.2, C_LABEL, align='center', bold=True)
        # Ratings (vertical text left of the jacks)
        for (tx, ty, txt) in ((112.5, 29.0, 'CAT IV 600V  CAT III 1000V'), (112.5, 70.0, 'FUSED  MAX 10A')):
            cr.save()
            cr.translate(tx, ty)
            cr.rotate(-1.5708)
            self.text(cr, 0, 0, txt, 1.6, C_LABEL)
            cr.restore()
        # Bracket lines as on the device (V/COM share the first group, COM/A the second)
        cr.set_source_rgba(1, 1, 1, 0.35)
        cr.set_line_width(0.35)
        cr.move_to(114.5, 16.5)
        cr.line_to(114.5, 46.0)
        cr.stroke()
        cr.move_to(133.5, 36.0)
        cr.line_to(133.5, 67.0)
        cr.stroke()

    def draw_key_icon(self, cr, code, cx, cy):
        cr.save()
        cr.set_source_rgb(*C_STRIP_TEXT)
        cr.set_line_width(0.45)
        cr.set_line_join(cairo.LINE_JOIN_ROUND)
        if code == 0:  # house
            cr.move_to(cx - 2.2, cy + 0.2)
            cr.line_to(cx, cy - 2.0)
            cr.line_to(cx + 2.2, cy + 0.2)
            cr.stroke()
            cr.rectangle(cx - 1.6, cy, 3.2, 2.2)
            cr.stroke()
        elif code == 1:  # play / pause
            cr.move_to(cx - 2.2, cy - 1.8)
            cr.line_to(cx - 2.2, cy + 1.8)
            cr.line_to(cx + 0.2, cy)
            cr.close_path()
            cr.stroke()
            cr.rectangle(cx + 0.9, cy - 1.8, 0.6, 3.6)
            cr.rectangle(cx + 2.0, cy - 1.8, 0.6, 3.6)
            cr.fill()
        elif code == 2:  # circled A
            cr.arc(cx, cy, 2.2, 0, 6.2832)
            cr.stroke()
            self.text(cr, cx, cy + 1.0, 'A', 2.6, C_STRIP_TEXT, align='center', bold=True)
        else:  # 2x2 grid
            for i in (0, 1):
                for j in (0, 1):
                    cr.rectangle(cx - 2.2 + i * 2.5, cy - 2.2 + j * 2.5, 1.9, 1.9)
            cr.stroke()
        cr.restore()


def parse_state(line):
    """key=value tokens of the HY3131 Frontend line (conn="..." quoted)."""
    st = {}
    rest = line
    if ' conn="' in rest:
        rest, conn = rest.split(' conn="', 1)
        st['conn'] = conn.rstrip('"')
    for tok in rest.split():
        if '=' in tok:
            k, v = tok.split('=', 1)
            st[k] = v
    return st


class FrontendView(Gtk.DrawingArea):
    """Schematic of the DM40 front end: jacks -> relays -> divider / shunt / ohm network -> HY3131,
    with the route the firmware selected highlighted."""

    def __init__(self):
        super().__init__()
        self.st = {}
        self.set_content_height(250)
        self.set_draw_func(self.draw)

    def update(self, st):
        self.st = st
        self.queue_draw()

    def draw(self, area, cr, w, h):
        st = self.st
        on = rgb('#3584E4')
        off = rgb('#6E6E6E')
        hot = rgb('#F3B51E')
        txt = rgb('#E8E8E8')
        cr.set_source_rgb(*rgb('#1E1F22'))
        cr.rectangle(0, 0, w, h)
        cr.fill()
        if not st:
            cr.set_source_rgb(*txt)
            cr.move_to(12, h / 2)
            cr.show_text('waiting for the HY3131 state …')
            return
        pa = st.get('pa', '0')
        role = st.get('role', 'None')
        shunt = st.get('shunt', 'uA')
        mode = st.get('mode', '-1')
        rng = st.get('range', '0')
        rv = role in ('DcV', 'AcV')
        rmv = role in ('DcMv', 'AcMv', 'Freq', 'Temp')
        rohm = role in ('Ohm', 'Cont', 'Diode', 'CapOsc', 'CapRamp', 'CapDischarge')
        ra = role in ('DcA', 'AcA')
        cr.set_line_width(2)
        cr.select_font_face('Cantarell')
        cr.set_font_size(11)

        def line(x0, y0, x1, y1, active):
            cr.set_source_rgb(*(on if active else off))
            cr.move_to(x0, y0)
            cr.line_to(x1, y1)
            cr.stroke()

        def label(x, y, s, active=True, color=None):
            cr.set_source_rgb(*(color or (txt if active else off)))
            cr.move_to(x, y)
            cr.show_text(s)

        def box(x, y, bw, bh, s, active, fill=False):
            cr.set_source_rgb(*(on if active else off))
            cr.rectangle(x, y, bw, bh)
            if fill:
                cr.fill()
            else:
                cr.stroke()
            cr.set_source_rgb(*(txt if active else off))
            ext = cr.text_extents(s)
            cr.move_to(x + (bw - ext.width) / 2, y + bh / 2 + 4)
            cr.show_text(s)

        yv, ycom, ya = 40, 130, 200
        # jacks
        for y, name, col in ((yv, 'V Ω', rgb('#C8102E')), (ycom, 'COM', rgb('#111111')), (ya, 'A', rgb('#C8102E'))):
            cr.set_source_rgb(*col)
            cr.arc(22, y, 9, 0, 6.3)
            cr.fill()
            cr.set_source_rgb(*txt)
            cr.arc(22, y, 4, 0, 6.3)
            cr.fill()
            label(4, y + 24, name)
        vj = st.get('vjack', '0')
        ij = st.get('ijack', '0')
        label(36, yv - 14, '%s V' % vj, True, hot)
        label(36, ya - 14, '%s A' % ij, True, hot)
        # V path: relay PA
        line(31, yv, 70, yv, True)
        va = pa == '0'
        box(70, yv - 12, 54, 24, 'PA: V/A' if va else 'PA: Ω/mV', True, fill=False)
        # two routes out of the relay
        line(124, yv, 150, yv, va)
        line(124, yv, 150, yv + 32, not va)
        divs = {'0': '/10', '1': '/100', '2': '/1000', '3': '/10000'}
        box(150, yv - 12, 78, 24, 'divider ' + (divs.get(rng, '') if rv else ''), va and rv)
        box(150, yv + 20, 78, 24, 'Ω/mV network', (not va) and (rmv or rohm))
        line(228, yv, 262, yv, va and rv)
        line(228, yv + 32, 262, yv + 32, (not va) and (rmv or rohm))
        # ohm source
        if rohm:
            label(150, yv + 62, 'source %s V, Rref %s' % (st.get('vsrc', '?'), st.get('rref', '?')), True, hot)
            if 'vx' in st:
                label(150, yv + 76, 'node %s V' % st['vx'], True, hot)
        # A path: shunt
        line(31, ya, 70, ya, True)
        box(70, ya - 12, 90, 24, 'shunt %s (%s)' % (shunt, {'A': '10 mΩ', 'mA': '0.5 Ω', 'uA': '47.5 Ω'}.get(shunt, '')), ra)
        line(160, ya, 262, ya, ra)
        label(165, ya - 6, 'Vshunt %s V' % st.get('vshunt', '0'), ra, hot if ra else None)
        # COM
        line(31, ycom, 262, ycom, True)
        label(40, ycom - 6, 'COM / AGND', True)
        # HY3131 box
        x0 = 262
        cr.set_source_rgb(*on)
        cr.rectangle(x0, 14, max(60, w - x0 - 8), h - 28)
        cr.stroke()
        label(x0 + 8, 30, 'HY3131  %s  m%s r%s' % (role, mode, rng))
        y = 48
        for key, name in (('ad1', 'AD1 (24 bit)'), ('ad2', 'AD2'), ('ad3', 'AD3'), ('lpf', 'LPF'), ('rms', 'RMS'),
                          ('ctr', 'counter'), ('cmp', 'comparator'), ('op1', 'OP1'), ('op2', 'OP2')):
            active = st.get(key) == '1'
            cr.set_source_rgb(*(on if active else off))
            cr.rectangle(x0 + 8, y - 8, 8, 8)
            cr.fill()
            extra = ''
            if key == 'ad1' and active:
                extra = '  OSR %s%s' % (st.get('osr1', ''), ' chop' if st.get('chop') == '1' else '')
            if key == 'op1' and active:
                extra = '  + %s' % st.get('sop1p', '')
            if key == 'cmp' and active:
                extra = '  in %s%s' % (st.get('scmpi', ''), '  CMP pin HIGH (beep)' if st.get('cmppin') == '1' else '')
            label(x0 + 22, y, name + extra, active)
            y += 15
        label(x0 + 8, y + 2, 'AD1 in %s-%s ref %s-%s' % (st.get('sad1fp', ''), st.get('sad1fn', ''), st.get('sad1rh', ''), st.get('sad1rl', '')), True)
        label(x0 + 8, y + 16, 'SAD1I %s  SDIO %s  SMODE %s  SFUVR %s  PC7 %s' % (
            st.get('sad1i', ''), st.get('sdio', ''), st.get('smode', ''), st.get('sfuvr', ''), st.get('pc7', '')), True)
        label(x0 + 8, y + 30, 'AD1 %s  period %s s  die %s C' % (st.get('ad1code', ''), st.get('period', ''), st.get('die', '')), True)


class GtkApp(Adw.Application):
    def __init__(self, args, extra, emulator_cls):
        super().__init__(application_id='win.xcvb.dm40emu')
        self.args, self.extra, self.emulator_cls = args, extra, emulator_cls
        self.emu = None
        self.starting = False
        self.key_down = {}
        self.rate_hist = []
        self.last_image = None
        self.connect('activate', self.on_activate)
        self.connect('shutdown', lambda *a: self.stop_emulator())

    # ---- window ----
    def on_activate(self, app):
        self.win = Adw.ApplicationWindow(application=self, title='DM40 emulator')
        self.win.set_default_size(1120, 700)
        self.title = Adw.WindowTitle(title='DM40 emulator', subtitle='starting…')
        header = Adw.HeaderBar()
        header.set_title_widget(self.title)

        self.fw = Gtk.DropDown.new_from_strings(['stock', 'fwrs', 'chain'])
        self.fw.set_selected(['stock', 'fwrs', 'chain'].index(self.args.mode))
        self.fw.set_tooltip_text('firmware to run')
        self.fw.connect('notify::selected', lambda *a: self.restart())
        header.pack_start(self.fw)
        self.fast = Gtk.ToggleButton(label='Fast')
        self.fast.set_tooltip_text('PerformanceInMips 50: about 2x faster, UI work takes longer in virtual time')
        self.fast.set_active(self.args.fast)
        self.fast.connect('toggled', lambda *a: self.restart())
        header.pack_start(self.fast)
        self.play = Gtk.ToggleButton(icon_name='media-playback-pause-symbolic')
        self.play.set_tooltip_text('pause / resume (space)')
        self.play.connect('toggled', self.on_pause_toggled)
        header.pack_start(self.play)
        restart = Gtk.Button(icon_name='view-refresh-symbolic')
        restart.set_tooltip_text('restart the emulator')
        restart.connect('clicked', lambda *a: self.restart())
        header.pack_start(restart)

        shot = Gtk.Button(icon_name='camera-photo-symbolic')
        shot.set_tooltip_text('screenshot to emu/out/ (s)')
        shot.connect('clicked', lambda *a: self.screenshot())
        header.pack_end(shot)
        self.side_btn = Gtk.ToggleButton(icon_name='sidebar-show-right-symbolic')
        self.side_btn.set_tooltip_text('inputs sidebar (F9)')
        header.pack_end(self.side_btn)

        self.panel = FrontPanel(self.on_panel_event)
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        content.append(header)
        self.split = Adw.OverlaySplitView()
        self.split.set_sidebar_position(Gtk.PackType.END)
        self.split.set_min_sidebar_width(340)
        self.split.set_max_sidebar_width(400)
        self.split.set_sidebar_width_fraction(0.3)
        self.split.set_show_sidebar(False)
        self.split.set_sidebar(self.build_sidebar())
        self.split.set_content(self.panel)
        self.side_btn.bind_property('active', self.split, 'show-sidebar', 1 | 2)  # bidirectional | sync
        content.append(self.split)
        self.win.set_content(content)

        keys = Gtk.EventControllerKey()
        keys.connect('key-pressed', self.on_key_pressed)
        keys.connect('key-released', self.on_key_released)
        self.win.add_controller(keys)
        self.win.connect('close-request', self.on_close)
        for sig in (signal.SIGINT, signal.SIGTERM):
            GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, sig, self.on_close)
        self.win.present()
        self.start_emulator()
        GLib.timeout_add(33, self.refresh)

    def build_sidebar(self):
        page = Adw.PreferencesPage()
        # DMM jacks: what is connected to V-ohm / COM / A (DM40 front-end model, emu/hy3131/README.md).
        # The firmware's relays and HY3131 switches route it; the schematic below shows the route.
        g = Adw.PreferencesGroup(title='DMM jacks', description='what is connected to V Ω / COM / A (Enter applies)')
        self.v_kind = Adw.ComboRow(title='V Ω / COM')
        self.v_kinds = ['nothing', 'DC voltage source', 'AC voltage source', 'resistor', 'capacitor', 'diode',
                        'thermocouple (type K)', 'short', 'current source (wrong jack)']
        self.v_kind.set_model(Gtk.StringList.new(self.v_kinds))
        self.v_kind.set_selected(1)
        self.v_kind.connect('notify::selected', lambda *a: self.dmm_fields_changed())
        g.add(self.v_kind)
        self.v_val = self.entry(g, 'Value', '1.234')
        self.v_ac = self.entry(g, 'AC, V rms', '0')
        self.v_f = self.entry(g, 'Frequency, Hz', '50')
        self.v_shape = Adw.ComboRow(title='Shape')
        self.v_shape.set_model(Gtk.StringList.new(['sine', 'square', 'triangle']))
        self.v_shape.connect('notify::selected', lambda *a: self.apply_dmm())
        g.add(self.v_shape)
        self.v_duty = self.entry(g, 'Duty 0..1', '0.5')
        self.v_rs = self.entry(g, 'Source resistance, ohm', '0')
        self.v_rev = Adw.SwitchRow(title='Reversed (cathode at V)')
        self.v_rev.connect('notify::active', lambda *a: self.apply_dmm())
        g.add(self.v_rev)
        self.a_kind = Adw.ComboRow(title='A / COM')
        self.a_kinds = ['nothing', 'DC current source', 'AC current source', 'voltage source (wrong jack)']
        self.a_kind.set_model(Gtk.StringList.new(self.a_kinds))
        self.a_kind.connect('notify::selected', lambda *a: self.dmm_fields_changed())
        g.add(self.a_kind)
        self.a_val = self.entry(g, 'Value', '0.0123')
        self.a_ac = self.entry(g, 'AC, A rms', '0')
        self.a_f = self.entry(g, 'Frequency, Hz', '50')
        self.a_compl = self.entry(g, 'Compliance, V', '10')
        self.cj = self.entry(g, 'Cold junction (jacks), C', '25')
        self.die = self.entry(g, 'HY3131 die temperature, C', '25')
        self.die_follows = Adw.SwitchRow(title='Die temperature = cold junction')
        self.die_follows.set_active(True)
        self.die_follows.connect('notify::active', lambda *a: self.dmm_fields_changed())
        g.add(self.die_follows)
        row = Adw.ActionRow(title='Connection')
        self.dmm_spec = row
        b = Gtk.Button(label='Apply', valign=Gtk.Align.CENTER)
        b.connect('clicked', lambda *a: self.apply_dmm())
        row.add_suffix(b)
        g.add(row)
        self.dmm_cmd = Adw.EntryRow(title='HY3131 monitor command')
        self.dmm_cmd.set_text('sysbus.hy3131 SetInput "vsrc:1.234"')
        self.dmm_cmd.connect('entry-activated', lambda r: self.send(r.get_text()))
        g.add(self.dmm_cmd)
        page.add(g)
        self.group_dmm = g
        # Front end: the route the firmware selected (relays, HY3131 switches), live from the state file
        g = Adw.PreferencesGroup(title='Front end', description='relays and HY3131 route chosen by the firmware')
        self.fe_view = FrontendView()
        g.add(self.fe_view)
        self.fe_row = Adw.ActionRow(title='state', subtitle='-')
        self.fe_row.set_subtitle_lines(4)
        g.add(self.fe_row)
        page.add(g)
        self.fe_state_path = os.path.join(run.OUT, 'live-frontend-%d.txt' % os.getpid())
        self.fe_state_sent = None
        self.fe_last = ''
        self.dmm_fields_changed()
        # Scope input
        g = Adw.PreferencesGroup(title='Scope input', description='MCX scope jack, ScopeFeeder')
        self.wave = Adw.ComboRow(title='Waveform')
        self.wave.set_model(Gtk.StringList.new(['sine', 'square', 'triangle', 'ramp', 'dc', 'noise']))
        g.add(self.wave)
        self.freq = Adw.SpinRow.new_with_range(0.1, 20e6, 1)
        self.freq.set_title('Frequency')
        self.freq.set_subtitle('Hz')
        self.freq.set_digits(1)
        self.freq.set_value(1000)
        g.add(self.freq)
        self.vpp = Adw.SpinRow.new_with_range(0, 100, 0.01)
        self.vpp.set_title('Amplitude')
        self.vpp.set_subtitle('V peak-to-peak')
        self.vpp.set_digits(3)
        self.vpp.set_value(1.0)
        g.add(self.vpp)
        self.voff = Adw.SpinRow.new_with_range(-50, 50, 0.01)
        self.voff.set_title('Offset')
        self.voff.set_subtitle('V')
        self.voff.set_digits(3)
        self.voff.set_value(0.0)
        g.add(self.voff)
        row = Adw.ActionRow(title='Apply waveform')
        b = Gtk.Button(label='Apply', valign=Gtk.Align.CENTER)
        b.connect('clicked', lambda *a: self.apply_wave())
        row.add_suffix(b)
        g.add(row)
        page.add(g)
        self.group_scope = g
        # Generator / USB
        g = Adw.PreferencesGroup(title='Generator and USB', description='MCX generator jack, USB-C')
        r = Adw.ActionRow(title='Generator output', subtitle='DAC/TIM6/TIM12 registers run; no analog output is modelled yet')
        g.add(r)
        r = Adw.ActionRow(title='USB', subtitle='stub: the device never enumerates')
        g.add(r)
        page.add(g)
        self.group_gen = g
        # Monitor
        g = Adw.PreferencesGroup(title='Renode monitor')
        self.mon = Adw.EntryRow(title='command')
        self.mon.connect('entry-activated', lambda r: self.send(r.get_text()))
        g.add(self.mon)
        self.reply = Adw.ActionRow(title='last reply')
        g.add(self.reply)
        page.add(g)
        return page

    # ---- emulator ----
    def extra_args(self):
        extra = list(self.extra)
        if self.args.app:
            extra += ['--app', self.args.app]
        if self.fast.get_active():
            extra += ['--fast']
        return extra

    def start_emulator(self):
        if self.starting:
            return
        self.starting = True
        mode = ['stock', 'fwrs', 'chain'][self.fw.get_selected()]
        self.title.set_subtitle('building / starting %s …' % mode)
        self.t_start = time.time()
        self.rate_hist = []
        extra = self.extra_args()

        def work():
            try:
                emu = self.emulator_cls(mode, extra)
            except Exception as e:  # build failure etc.
                emu = e
            GLib.idle_add(self.started, emu)
        threading.Thread(target=work, daemon=True).start()

    def started(self, emu):
        self.starting = False
        if isinstance(emu, Exception):
            self.title.set_subtitle('start failed: %s' % emu)
            return False
        self.emu = emu
        self.play.set_active(False)
        return False

    def stop_emulator(self):
        if self.emu:
            e, self.emu = self.emu, None
            e.stop()

    def restart(self):
        if self.starting:
            return
        self.stop_emulator()
        self.start_emulator()

    def on_close(self, *a):
        self.stop_emulator()
        self.quit()
        return True

    def send(self, cmd):
        if self.emu:
            self.emu.send(cmd)

    # ---- display ----
    def refresh(self):
        if not self.emu:
            if self.starting:
                self.title.set_subtitle('starting … %.0f s' % (time.time() - self.t_start))
            return True
        if not self.emu.alive():
            self.title.set_subtitle('Renode exited (see %s)' % run.rel(self.emu.log_path))
            return True
        f = self.emu.frame()
        if f is None:
            self.title.set_subtitle('waiting for the emulator … %.0f s' % (time.time() - self.t_start))
            return True
        seq, vus, pix, rgbarr = f
        if seq != getattr(self, 'last_seq', None):
            self.last_seq = seq
            self.last_rgb = rgbarr
            data = GLib.Bytes.new(rgbarr.tobytes())
            self.panel.texture = Gdk.MemoryTexture.new(W, H, Gdk.MemoryFormat.R8G8B8, data, W * 3)
            self.panel.queue_draw()
        now = time.time()
        self.rate_hist.append((now, vus))
        self.rate_hist = [p for p in self.rate_hist if now - p[0] < 2.0]
        ratio = 0.0
        if len(self.rate_hist) > 1 and now - self.rate_hist[0][0] > 0.2:
            ratio = (vus - self.rate_hist[0][1]) / 1e6 / (now - self.rate_hist[0][0])
        mode = ['stock', 'fwrs', 'chain'][self.fw.get_selected()]
        state = 'paused' if self.play.get_active() else '%.2fx real time' % ratio
        self.title.set_subtitle('%s · virtual %.2f s · %s' % (mode, vus / 1e6, state))
        if self.emu.last_reply:
            self.reply.set_subtitle(self.emu.last_reply)
        self.fe_poll()
        return True

    def screenshot(self):
        rgbarr = getattr(self, 'last_rgb', None)
        if rgbarr is None:
            return
        from PIL import Image
        path = os.path.join(run.OUT, 'live-%s.png' % time.strftime('%Y%m%d-%H%M%S'))
        Image.fromarray(rgbarr, 'RGB').save(path)
        self.title.set_subtitle('saved ' + run.rel(path))

    # ---- input ----
    def on_panel_event(self, kind, data):
        if kind == 'press':
            self.send('sysbus.i2c3.touch Press %d %d' % data)
        elif kind == 'move':
            self.send('sysbus.i2c3.touch Move %d %d' % data)
        elif kind == 'release':
            self.send('sysbus.i2c3.touch Release')
        elif kind == 'touch':
            self.send('sysbus.i2c3.touch Touch "%s"' % ';'.join('%d,%d' % p for p in data))
        elif kind == 'keydown':
            self.send('sysbus.i2c3.touch KeyDown %d' % data)
        elif kind == 'keyup':
            self.send('sysbus.i2c3.touch KeyUp')
        elif kind == 'power':
            self.send('sysbus.gpioPortD OnGPIO 6 %s' % ('true' if data else 'false'))
        elif kind == 'jack':
            self.split.set_show_sidebar(True)
            target = {'v': self.group_dmm, 'com': self.group_dmm, 'a': self.group_dmm,
                      'scope': self.group_scope, 'gen': self.group_gen, 'usb': self.group_gen,
                      'led': self.group_gen}[data]
            target.grab_focus()

    def entry(self, group, title, text):
        e = Adw.EntryRow(title=title)
        e.set_text(text)
        e.connect('entry-activated', lambda r: self.apply_dmm())
        group.add(e)
        return e

    V_DEFAULTS = {'DC voltage source': '1.234', 'AC voltage source': '0', 'resistor': '4700', 'capacitor': '100e-9',
                  'diode': '0.65', 'thermocouple (type K)': '100', 'current source (wrong jack)': '0.001'}
    A_DEFAULTS = {'DC current source': '0.0123', 'AC current source': '0', 'voltage source (wrong jack)': '0.5'}

    def dmm_fields_changed(self):
        """Show only the fields of the chosen sources (with a typical value when the kind changes), then apply."""
        vk = self.v_kinds[self.v_kind.get_selected()]
        ak = self.a_kinds[self.a_kind.get_selected()]
        if vk != getattr(self, 'last_vk', vk) and vk in self.V_DEFAULTS:
            self.v_val.set_text(self.V_DEFAULTS[vk])
        if ak != getattr(self, 'last_ak', ak) and ak in self.A_DEFAULTS:
            self.a_val.set_text(self.A_DEFAULTS[ak])
        self.last_vk, self.last_ak = vk, ak
        src = vk in ('DC voltage source', 'AC voltage source', 'current source (wrong jack)')
        self.v_val.set_visible(vk not in ('nothing', 'short'))
        self.v_val.set_title({'DC voltage source': 'DC, V', 'AC voltage source': 'DC offset, V', 'resistor': 'Resistance, ohm',
                              'capacitor': 'Capacitance, F', 'diode': 'Forward voltage, V',
                              'thermocouple (type K)': 'Hot junction, C', 'current source (wrong jack)': 'DC, A'}.get(vk, 'Value'))
        self.v_ac.set_visible(vk == 'AC voltage source')
        self.v_f.set_visible(vk == 'AC voltage source')
        self.v_shape.set_visible(vk == 'AC voltage source')
        self.v_duty.set_visible(vk == 'AC voltage source' and self.v_shape.get_selected() == 1)
        self.v_rs.set_visible(src and vk != 'current source (wrong jack)')
        self.v_rev.set_visible(vk == 'diode')
        self.a_val.set_visible(ak != 'nothing')
        self.a_val.set_title({'DC current source': 'DC, A', 'AC current source': 'DC offset, A',
                              'voltage source (wrong jack)': 'DC, V'}.get(ak, 'Value'))
        self.a_ac.set_visible(ak == 'AC current source')
        self.a_f.set_visible(ak == 'AC current source')
        self.a_compl.set_visible(ak in ('DC current source', 'AC current source'))
        self.die.set_visible(not self.die_follows.get_active())
        self.apply_dmm()

    @staticmethod
    def num(entry, default):
        t = entry.get_text().strip().replace(',', '.')
        try:
            float(t)
            return t
        except ValueError:
            return default

    def dmm_spec_text(self):
        vk = self.v_kinds[self.v_kind.get_selected()]
        ak = self.a_kinds[self.a_kind.get_selected()]
        parts = ['open']
        v = self.num(self.v_val, '0')
        f = self.num(self.v_f, '50')
        shape = self.v_shape.get_selected_item().get_string()
        if vk == 'DC voltage source':
            parts.append('vsrc:%s,rs=%s' % (v, self.num(self.v_rs, '0')))
        elif vk == 'AC voltage source':
            parts.append('vsrc:%s,ac=%s,f=%s,shape=%s,duty=%s,rs=%s' % (
                v, self.num(self.v_ac, '0'), f, shape, self.num(self.v_duty, '0.5'), self.num(self.v_rs, '0')))
        elif vk == 'resistor':
            parts.append('r:%s' % v)
        elif vk == 'capacitor':
            parts.append('c:%s' % v)
        elif vk == 'diode':
            parts.append('diode:%s%s' % (v, ',rev=1' if self.v_rev.get_active() else ''))
        elif vk == 'thermocouple (type K)':
            parts.append('tc:%s' % v)
        elif vk == 'short':
            parts.append('r:0')
        elif vk == 'current source (wrong jack)':
            parts.append('isrcv:%s' % v)
        a = self.num(self.a_val, '0')
        if ak == 'DC current source':
            parts.append('isrc:%s,compl=%s' % (a, self.num(self.a_compl, '10')))
        elif ak == 'AC current source':
            parts.append('isrc:%s,ac=%s,f=%s,compl=%s' % (a, self.num(self.a_ac, '0'), self.num(self.a_f, '50'), self.num(self.a_compl, '10')))
        elif ak == 'voltage source (wrong jack)':
            parts.append('avsrc:%s' % a)
        parts.append('cj:%s' % self.num(self.cj, '25'))
        parts.append('die:none' if self.die_follows.get_active() else 'die:%s' % self.num(self.die, '25'))
        return ';'.join(parts)

    def apply_dmm(self):
        spec = self.dmm_spec_text()
        self.dmm_spec.set_subtitle(spec)
        self.send('sysbus.hy3131 SetInput "%s"' % spec)

    def fe_poll(self):
        """Front-end state: ask the peripheral to write its state file once, then read it."""
        if not self.emu or not self.emu.alive():
            return
        if self.fe_state_sent is not self.emu:
            self.fe_state_sent = self.emu
            self.send('sysbus.hy3131 StateFile @%s' % self.fe_state_path)
            self.apply_dmm()
            return
        try:
            with open(self.fe_state_path) as f:
                line = f.read().strip()
        except OSError:
            return
        if line and line != self.fe_last:
            self.fe_last = line
            st = parse_state(line)
            self.fe_view.update(st)
            self.fe_row.set_subtitle('%s  shunt %s  PC7 %s · %s mode %s range %s · AD1 %s AD2 %s AD3 %s LPF %s RMS %s CTR %s · '
                                     'Vjack %s V  Ijack %s A%s' % (
                                         'V/A path' if st.get('pa') == '0' else 'Ω/mV network', st.get('shunt'), st.get('pc7'),
                                         st.get('role'), st.get('mode'), st.get('range'), st.get('ad1'), st.get('ad2'), st.get('ad3'),
                                         st.get('lpf'), st.get('rms'), st.get('ctr'), st.get('vjack'), st.get('ijack'),
                                         ('  node %s V' % st['vx']) if 'vx' in st else ''))

    def apply_wave(self):
        shape = self.wave.get_selected_item().get_string()
        self.send('sysbus.scopefeed SetWave "%s" %g %g %g' % (
            shape, self.freq.get_value(), self.vpp.get_value(), self.voff.get_value()))

    def on_key_pressed(self, ctrl, keyval, keycode, state):
        name = Gdk.keyval_name(keyval) or ''
        low = name.lower()
        if low in ('escape',) or (low == 'c' and state & Gdk.ModifierType.CONTROL_MASK):
            self.on_close()
            return True
        if isinstance(self.win.get_focus(), (Gtk.Text, Gtk.Entry)):
            return False  # typing in the sidebar
        if low in KEY_CHARS:
            code = KEY_CHARS[low]
            if not self.key_down.get(code):
                self.key_down[code] = True
                self.panel.pressed_key = code
                self.panel.queue_draw()
                self.send('sysbus.i2c3.touch KeyDown %d' % code)
            return True
        if low == 'p':
            if not self.key_down.get('p'):
                self.key_down['p'] = True
                self.panel.pressed_power = True
                self.panel.queue_draw()
                self.send('sysbus.gpioPortD OnGPIO 6 true')
            return True
        if low == 'f2':
            self.send('sysbus.i2c3.touch MultiSwipe 2 240 80 0 120 300')
            return True
        if low == 'f3':
            self.send('sysbus.i2c3.touch MultiSwipe 3 240 80 0 120 300')
            return True
        if low == 'f9':
            self.split.set_show_sidebar(not self.split.get_show_sidebar())
            return True
        if low == 'space':
            self.play.set_active(not self.play.get_active())
            return True
        if low == 's':
            self.screenshot()
            return True
        return False

    def on_key_released(self, ctrl, keyval, keycode, state):
        low = (Gdk.keyval_name(keyval) or '').lower()
        if low in KEY_CHARS and self.key_down.pop(KEY_CHARS[low], None):
            self.panel.pressed_key = None
            self.panel.queue_draw()
            self.send('sysbus.i2c3.touch KeyUp')
        elif low == 'p' and self.key_down.pop('p', None):
            self.panel.pressed_power = False
            self.panel.queue_draw()
            self.send('sysbus.gpioPortD OnGPIO 6 false')

    def on_pause_toggled(self, btn):
        if not self.emu:
            return
        paused = btn.get_active()
        self.emu.paused = paused
        btn.set_icon_name('media-playback-start-symbolic' if paused else 'media-playback-pause-symbolic')
        self.send('pause' if paused else 'start')


def run_app(args, extra, emulator_cls):
    app = GtkApp(args, extra, emulator_cls)
    try:
        return app.run([])
    finally:
        app.stop_emulator()
