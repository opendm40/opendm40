"""GTK4/libadwaita window for emu/live.py: the DM40 front panel.

Geometry is in millimetres of the real device (140 x 83 mm, manual figure
2.1.1): a 480x320 display of 73.5 x 49 mm in a black glass with the
MENU/RUN/AUTO/MF touch strip on its left, power button on the left edge,
V-Ohm / COM / A banana jacks on the right (positions, colours and legends from
the manual figure and product photos). The top edge (USB-C with the charge LED,
MCX generator OUT, MCX scope OSC) is drawn as a separate top view above the
front view, like an engineering drawing (y < 0 in the same mm frame). The display is rendered at an integer device-pixel
scale k (nearest neighbour), the rest with Cairo at 480k/73.5 px per mm.
"""
import itertools
import os
import re
import signal
import sys
import threading
import time
from math import cos, sin

import gi

gi.require_version('Gtk', '4.0')
gi.require_version('Adw', '1')
gi.require_version('Gsk', '4.0')
gi.require_version('Graphene', '1.0')
gi.require_version('Pango', '1.0')
gi.require_version('PangoCairo', '1.0')
import cairo  # noqa: E402
from gi.repository import Adw, Gdk, GLib, Graphene, Gsk, Gtk, Pango, PangoCairo  # noqa: E402

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import run  # noqa: E402

W, H = 480, 320
DEV_W, DEV_H = 140.0, 83.0            # device body, mm
DISP = (22.5, 15.5, 73.5, 49.0)       # display x, y, w, h (mm)
GLASS = (8.0, 8.5, 90.0, 66.0)        # black glass (keys + logo + display)
STRIP = (11.0, 15.0, 9.0, 52.0)       # touch key strip, 4 cells
POWER = (0.0, 30.0, 6.5, 14.0)        # power button on the left edge
# Top view of the top edge (photo of the edge: USB-C in a black recess with the LED pinhole at
# its right end, then MCX OUT and MCX OSC; left-right as in the front view), above the front view.
TOPV_H, TOPV_GAP = 18.0, 4.0          # case depth, gap to the front view (mm)
TOPV_Y = -(TOPV_GAP + TOPV_H)         # back of the case; the front face is at y = -TOPV_GAP
TOPV_CY = TOPV_Y + 10.0               # connector axis
SHOW_TOP_VIEW = False                  # the top-edge view (USB / OUT / OSC): off, the cards cover them
PANEL_Y0, PANEL_H = (TOPV_Y, DEV_H - TOPV_Y) if SHOW_TOP_VIEW else (0.0, DEV_H)
USB_RECESS = (16.2, TOPV_CY - 3.7, 17.2, 7.4)
USB_PORT = (19.45, TOPV_CY - 1.5, 8.9, 3.0)
LED_C = (32.0, TOPV_CY + 0.2)
MCX = {'gen': (44.2, TOPV_CY), 'scope': (63.3, TOPV_CY)}
MCX_R = 4.15                          # black recess around the MCX
# hit rects; 'led' before 'usb' (it sits in the USB recess)
TOP = {'led': (30.4, USB_RECESS[1], 3.0, USB_RECESS[3]),
       'usb': (USB_RECESS[0], USB_RECESS[1], 30.4 - USB_RECESS[0], USB_RECESS[3]),
       'gen': (MCX['gen'][0] - MCX_R, TOPV_CY - MCX_R, 2 * MCX_R, 2 * MCX_R),
       'scope': (MCX['scope'][0] - MCX_R, TOPV_CY - MCX_R, 2 * MCX_R, 2 * MCX_R)}
JACKS = {'v': (126.1, 22.4), 'com': (126.1, 41.1), 'a': (126.1, 60.5)}
JACK_R = 4.5                          # black collar
FONT = 'Liberation Sans'              # all printed legends (Helvetica-like, as on the case)
KEY_NAMES = ['MENU', 'RUN', 'AUTO', 'MF']
WIN_W, WIN_H = 1440, 900            # default window size: device (k = 1: 915 x 687 px) + input cards beside / under it
SIDE_MIN_W = 340                     # the right-hand card column is never narrower
PANEL_MARGIN, CARD_GAP = 6, 12       # px around the device picture, between cards
KEY_CHARS = {'m': 0, 'r': 1, 'a': 2, 'f': 3}
PX_PER_MM_1X = W / DISP[2]           # 6.53 px/mm at k = 1


def rgb(h):
    return tuple(int(h[i:i + 2], 16) / 255 for i in (1, 3, 5))


C_HOLSTER = rgb('#F7B728')
C_HOLSTER_DARK = rgb('#C98B0C')
C_HOLSTER_LIGHT = rgb('#FCC948')
C_HOLSTER_SHADOW = rgb('#A87200')
C_HOLSTER_EMBOSS = rgb('#FFF2CC')     # legends moulded into the case (top edge)
C_RECESS = rgb('#141414')
C_FACE = rgb('#3A3A3B')               # matte grey face around the glass / jacks
C_GLASS = rgb('#0A0A0B')
C_JACK_RED = rgb('#A8262A')
C_JACK_RED_HI = rgb('#D0564C')
C_LEGEND = rgb('#E6E6E6')             # white print on the face
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
        self.ink = None               # list -> draw_panel records ink boxes (overlap check)
        self.model = 'A'              # printed model letter (the header's model dropdown)
        self.want_k = 1.0             # scale the window picked (GtkApp.on_body_tick; fractional)
        self.set_margin_start(PANEL_MARGIN)
        self.set_margin_end(PANEL_MARGIN)
        self.set_margin_top(PANEL_MARGIN)
        self.set_margin_bottom(PANEL_MARGIN)
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
        k = max(0.5, min(aw * sf / (DEV_W * PX_PER_MM_1X), ah * sf / (PANEL_H * PX_PER_MM_1X)))
        s = PX_PER_MM_1X * k / sf
        ox = (aw - DEV_W * s) / 2
        oy = (ah - PANEL_H * s) / 2 - PANEL_Y0 * s
        return k, s, ox, oy

    def do_measure(self, orientation, for_size):
        # minimum: half scale (the Paned handle may shrink the device), natural: the scale picked
        sf = self.get_scale_factor()
        nat = int((DEV_W if orientation == Gtk.Orientation.HORIZONTAL else PANEL_H) * PX_PER_MM_1X * self.want_k / sf) + 1
        if orientation == Gtk.Orientation.VERTICAL:
            # the left side scrolls (the signal-flow diagram is under the device): never squeeze the height
            return nat, nat, -1, -1
        return int(DEV_W * PX_PER_MM_1X * 0.5 / sf) + 1, nat, -1, -1

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
            if (mx - cx) ** 2 + (my - cy) ** 2 <= (JACK_R + 0.8) ** 2:
                return 'jack', name
        for name, r in (TOP.items() if SHOW_TOP_VIEW else ()):
            if self.inside((r[0] - 0.5, r[1] - 0.8, r[2] + 1, r[3] + 1.6), mx, my):
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
        # Display: nearest-neighbour at an integer device-pixel scale, smooth otherwise.
        dx = ox + DISP[0] * s
        dy = oy + DISP[1] * s
        dw, dh = DISP[2] * s, DISP[3] * s
        if self.texture is not None:
            filt = Gsk.ScalingFilter.NEAREST if abs(k - round(k)) < 0.01 else Gsk.ScalingFilter.LINEAR
            snapshot.append_scaled_texture(self.texture, filt,
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

    def text(self, cr, x, y, txt, size, color=C_LABEL, align='left', bold=False, face=FONT, ink=True):
        """Draw txt with its baseline at y; returns the advance width (mm)."""
        cr.save()
        cr.select_font_face(face, cairo.FONT_SLANT_NORMAL,
                            cairo.FONT_WEIGHT_BOLD if bold else cairo.FONT_WEIGHT_NORMAL)
        cr.set_font_size(size)
        ext = cr.text_extents(txt)
        if align == 'center':
            x -= ext.x_advance / 2
        elif align == 'right':
            x -= ext.x_advance
        cr.new_path()
        cr.move_to(x, y)
        cr.set_source_rgb(*color)
        cr.show_text(txt)
        cr.new_path()                 # show_text leaves a current point: the next arc would join it
        if ink:
            self.mark(cr, txt, x + ext.x_bearing, y + ext.y_bearing, ext.width, ext.height)
        cr.restore()
        return ext.x_advance

    @staticmethod
    def text_width(cr, txt, size, bold=True, face=FONT):
        cr.save()
        cr.select_font_face(face, cairo.FONT_SLANT_NORMAL,
                            cairo.FONT_WEIGHT_BOLD if bold else cairo.FONT_WEIGHT_NORMAL)
        cr.set_font_size(size)
        w = cr.text_extents(txt).x_advance
        cr.restore()
        return w

    def mark(self, cr, name, x, y, w, h):
        """Record an ink box (device px) for the overlap check (tools: see the commit message)."""
        if self.ink is None:
            return
        pts = [cr.user_to_device(px, py) for px, py in ((x, y), (x + w, y), (x, y + h), (x + w, y + h))]
        xs, ys = [p[0] for p in pts], [p[1] for p in pts]
        self.ink.append((name, min(xs), min(ys), max(xs), max(ys)))

    def draw_panel(self, cr):
        cr.new_path()
        if SHOW_TOP_VIEW:
            self.draw_top_view(cr)
        # Holster (yellow bumper) and the dark grey face
        cr.set_source_rgb(*C_HOLSTER)
        self.rrect(cr, 0, 0, DEV_W, DEV_H, 10)
        cr.fill()
        cr.set_source_rgb(*C_HOLSTER_DARK)
        cr.set_line_width(0.5)
        self.rrect(cr, 0.25, 0.25, DEV_W - 0.5, DEV_H - 0.5, 9.8)
        cr.stroke()
        cr.set_source_rgba(*C_HOLSTER_DARK, 0.7)    # inner lip of the bumper
        cr.set_line_width(0.9)
        self.rrect(cr, 4.0, 5.5, DEV_W - 8.0, DEV_H - 10.5, 5)
        cr.stroke()
        cr.set_source_rgb(*C_FACE)
        self.rrect(cr, 4.5, 6.0, DEV_W - 9.0, DEV_H - 11.5, 4.5)
        cr.fill()
        # Glass: logo, model, touch keys, display
        cr.set_source_rgb(*C_GLASS)
        self.rrect(cr, *GLASS, 2.5)
        cr.fill()
        self.draw_logo(cr, 24.0, 11.6)
        self.text(cr, 26.3, 12.85, '正点原子', 3.1, C_STRIP_TEXT, bold=True, face='Noto Sans CJK SC')
        self.text(cr, GLASS[0] + GLASS[2] - 2.5, 12.85, 'DM40' + self.model, 2.6, C_STRIP_TEXT, align='right', bold=True)
        cell = STRIP[3] / 4
        for i, name in enumerate(KEY_NAMES):
            x, y = STRIP[0], STRIP[1] + i * cell
            if self.pressed_key == i:
                cr.set_source_rgba(*C_PRESSED, 0.55)
                self.rrect(cr, x + 0.4, y + 0.5, STRIP[2] - 0.8, cell - 1.0, 1.5)
                cr.fill()
            self.draw_key_icon(cr, i, x + STRIP[2] / 2, y + cell * 0.40)
            self.text(cr, x + STRIP[2] / 2, y + cell * 0.79, name, 2.15, C_STRIP_TEXT, align='center', bold=True)
        # Power button (left edge)
        px, py, pw, ph = POWER
        cr.set_source_rgb(*(C_PRESSED if self.pressed_power else C_BLACK))
        self.rrect(cr, px, py, pw, ph, 2.5)
        cr.fill()
        cr.new_path()
        cr.set_source_rgb(*C_LABEL)
        cr.set_line_width(0.5)
        cr.arc(px + pw / 2, py + ph / 2, 2.0, -1.1, 4.25)
        cr.stroke()
        cr.move_to(px + pw / 2, py + ph / 2 - 2.6)
        cr.line_to(px + pw / 2, py + ph / 2 - 0.4)
        cr.stroke()
        self.draw_jacks(cr)

    def draw_logo(self, cr, cx, cy):
        """Alientek's round logo (a white disc with a Z swoosh)."""
        cr.save()
        cr.new_path()
        cr.set_source_rgb(*C_STRIP_TEXT)
        cr.arc(cx, cy, 1.55, 0, 6.2832)
        cr.fill()
        cr.set_source_rgb(*C_GLASS)
        cr.set_line_width(0.42)
        cr.set_line_join(cairo.LINE_JOIN_ROUND)
        cr.move_to(cx - 0.85, cy - 0.65)
        cr.line_to(cx + 0.8, cy - 0.65)
        cr.line_to(cx - 0.8, cy + 0.65)
        cr.line_to(cx + 0.85, cy + 0.65)
        cr.stroke()
        cr.restore()
        self.mark(cr, 'logo', cx - 1.55, cy - 1.55, 3.1, 3.1)

    # ---- top edge (top view) ----
    def draw_top_view(self, cr):
        y0, h = TOPV_Y, TOPV_H
        # the case seen from above: rounded long edges (back at the top, front face at the bottom)
        g = cairo.LinearGradient(0, y0, 0, y0 + h)
        for t, c in ((0.0, C_HOLSTER_DARK), (0.16, C_HOLSTER), (0.55, C_HOLSTER_LIGHT), (0.86, C_HOLSTER), (1.0, C_HOLSTER_DARK)):
            g.add_color_stop_rgb(t, *c)
        self.rrect(cr, 0, y0, DEV_W, h, 5)
        cr.set_source(g)
        cr.fill()
        # rounded short ends
        g = cairo.LinearGradient(0, 0, DEV_W, 0)
        for t, a in ((0.0, 0.35), (0.05, 0.0), (0.95, 0.0), (1.0, 0.35)):
            g.add_color_stop_rgba(t, *C_HOLSTER_DARK, a)
        self.rrect(cr, 0, y0, DEV_W, h, 5)
        cr.set_source(g)
        cr.fill()
        cr.set_source_rgb(*C_HOLSTER_DARK)
        cr.set_line_width(0.3)
        self.rrect(cr, 0.15, y0 + 0.15, DEV_W - 0.3, h - 0.3, 4.9)
        cr.stroke()
        # USB-C in a black recess, charge LED pinhole at the recess' right end
        cr.set_source_rgb(*C_RECESS)
        self.rrect(cr, *USB_RECESS, 2.4)
        cr.fill()
        self.mark(cr, 'usb', *USB_RECESS)
        x, y, w, hh = USB_PORT
        cr.set_source_rgb(0.02, 0.02, 0.02)
        self.rrect(cr, x, y, w, hh, hh / 2)
        cr.fill_preserve()
        cr.set_source_rgb(0.62, 0.62, 0.64)
        cr.set_line_width(0.3)
        cr.stroke()
        cr.set_source_rgb(0.2, 0.2, 0.21)
        self.rrect(cr, x + 1.4, y + hh / 2 - 0.35, w - 2.8, 0.7, 0.35)
        cr.fill()
        lx, ly = LED_C
        glow = cairo.RadialGradient(lx, ly, 0, lx, ly, 1.3)
        glow.add_color_stop_rgba(0, 0.35, 0.65, 1.0, 0.9)
        glow.add_color_stop_rgba(1, 0.35, 0.65, 1.0, 0.0)
        cr.set_source(glow)
        cr.arc(lx, ly, 1.3, 0, 6.2832)
        cr.fill()
        cr.set_source_rgb(0.55, 0.8, 1.0)
        cr.arc(lx, ly, 0.45, 0, 6.2832)
        cr.fill()
        # MCX jacks: black recess, gold body with a bright rim, dielectric, centre pin
        for name, (cx, cy) in MCX.items():
            cr.set_source_rgb(*C_RECESS)
            cr.arc(cx, cy, MCX_R, 0, 6.2832)
            cr.fill()
            self.mark(cr, name, cx - MCX_R, cy - MCX_R, 2 * MCX_R, 2 * MCX_R)
            gold = cairo.RadialGradient(cx - 0.6, cy - 0.6, 0.2, cx, cy, 2.1)
            gold.add_color_stop_rgb(0, 0.93, 0.80, 0.50)
            gold.add_color_stop_rgb(1, 0.66, 0.50, 0.24)
            cr.set_source(gold)
            cr.arc(cx, cy, 2.1, 0, 6.2832)
            cr.fill()
            cr.set_source_rgb(0.88, 0.87, 0.82)
            cr.set_line_width(0.25)
            cr.arc(cx, cy, 2.0, 0, 6.2832)
            cr.stroke()
            cr.set_source_rgb(0.30, 0.26, 0.20)
            cr.arc(cx, cy, 1.15, 0, 6.2832)
            cr.fill()
            cr.set_source_rgb(0.95, 0.80, 0.45)
            cr.arc(cx, cy, 0.35, 0, 6.2832)
            cr.fill()
        # legends moulded into the case above the connectors
        ly = USB_RECESS[1] - 1.1
        for x, txt in ((USB_PORT[0] + USB_PORT[2] / 2, 'USB'), (MCX['gen'][0], 'OUT'), (MCX['scope'][0], 'OSC')):
            self.text(cr, x + 0.15, ly + 0.15, txt, 2.7, C_HOLSTER_SHADOW, align='center', ink=False)
            self.text(cr, x, ly, txt, 2.7, C_HOLSTER_EMBOSS, align='center')

    # ---- banana jacks and their legends ----
    def draw_jacks(self, cr):
        for name, (cx, cy) in JACKS.items():
            cr.new_path()
            collar = cairo.RadialGradient(cx - 1.2, cy - 1.6, 0.5, cx, cy, JACK_R)
            collar.add_color_stop_rgb(0, 0.16, 0.16, 0.16)
            collar.add_color_stop_rgb(1, 0.04, 0.04, 0.04)
            cr.set_source(collar)
            cr.arc(cx, cy, JACK_R, 0, 6.2832)
            cr.fill()
            cr.set_source_rgb(0.30, 0.30, 0.30)
            cr.set_line_width(0.2)
            cr.arc(cx, cy, JACK_R - 0.1, 0, 6.2832)
            cr.stroke()
            self.mark(cr, 'jack-' + name, cx - JACK_R, cy - JACK_R, 2 * JACK_R, 2 * JACK_R)
            ring, hi = ((0.11, 0.11, 0.11), (0.24, 0.24, 0.24)) if name == 'com' else (C_JACK_RED, C_JACK_RED_HI)
            cr.set_source_rgb(*ring)
            cr.arc(cx, cy, 3.0, 0, 6.2832)
            cr.fill()
            cr.set_source_rgb(*hi)
            cr.set_line_width(0.3)
            cr.arc(cx, cy, 2.6, 3.4, 5.6)
            cr.stroke()
            cr.set_source_rgb(0.02, 0.02, 0.02)
            cr.arc(cx, cy, 1.8, 0, 6.2832)
            cr.fill()
            cr.set_source_rgb(0.13, 0.13, 0.13)
            cr.arc(cx, cy, 0.8, 0, 6.2832)
            cr.fill()
        vx = JACKS['v'][0]
        sz = 3.1
        cap = 0.716 * sz              # Liberation Sans Bold cap height
        # line 1: V Ω  ·)))  ->|      line 2: -||- Hz °C
        wvo = self.text_width(cr, 'VΩ', sz)
        w1 = wvo + 0.55 + 1.9 + 0.6 + 2.6
        x = vx - w1 / 2
        b = 30.0
        x += self.text(cr, x, b, 'VΩ', sz, C_LEGEND, bold=True) + 0.55
        self.draw_beeper(cr, x, b, cap)
        x += 1.9 + 0.6
        self.draw_diode(cr, x, b, cap)
        whz = self.text_width(cr, 'Hz°C', sz)
        w2 = 2.2 + 0.6 + whz
        x = vx - w2 / 2
        b = 33.6
        self.draw_cap(cr, x, b, cap)
        self.text(cr, x + 2.8, b, 'Hz°C', sz, C_LEGEND, bold=True)
        self.text(cr, vx, 50.9, 'COM', 3.3, C_LEGEND, align='center', bold=True)
        self.text(cr, vx, 69.3, 'A mA uA', 3.2, C_LEGEND, align='center', bold=True)
        # rotated ratings, each between a pair of bracket lines that point at the jacks they apply to
        cat = self.vtext(cr, [(112.7, 'CAT IV 600V'), (115.0, 'CAT III 1000V')], 31.5, 2.0)
        self.draw_ground(cr, 117.7, 31.5)
        fus = self.vtext(cr, [(112.4, 'FUSED'), (114.9, 'MAX 10A')], 51.6, 2.0)
        xj = vx - JACK_R - 0.7
        self.bracket(cr, 'b1-top', 114.0, 31.5 - cat / 2 - 0.9, xj, JACKS['v'][1], up=True)
        self.bracket(cr, 'b1-bot', 114.0, 31.5 + cat / 2 + 0.9, xj, JACKS['com'][1] - 0.5, up=False)
        self.bracket(cr, 'b2-top', 113.7, 51.6 - fus / 2 - 0.9, xj, JACKS['com'][1] + 0.5, up=True)
        self.bracket(cr, 'b2-bot', 113.7, 51.6 + fus / 2 + 0.9, xj, JACKS['a'][1], up=False)

    def vtext(self, cr, cols, cy, size):
        """Columns of text rotated to read bottom-to-top, centred on y = cy; -> longest length (mm)."""
        n = max(self.text_width(cr, t, size) for _, t in cols)
        cap = 0.716 * size
        for x, t in cols:
            w = self.text_width(cr, t, size)
            cr.save()
            cr.translate(x + cap / 2, cy + w / 2)
            cr.rotate(-1.5708)
            self.text(cr, 0, 0, t, size, C_LEGEND, bold=True)
            cr.restore()
        return n

    def bracket(self, cr, name, x, y_end, xj, yj, up):
        """A legend bracket: vertical from (x, y_end) to the jack row yj, rounded corner, right to xj."""
        r = 1.2
        cr.save()
        cr.new_path()
        cr.set_source_rgb(*C_LEGEND)
        cr.set_line_width(0.35)
        cr.move_to(x, y_end)
        if up:
            cr.line_to(x, yj + r)
            cr.arc(x + r, yj + r, r, 3.14159, 4.71239)
        else:
            cr.line_to(x, yj - r)
            cr.arc_negative(x + r, yj - r, r, 3.14159, 1.5708)
        cr.line_to(xj, yj)
        cr.stroke()
        cr.restore()
        self.mark(cr, name, x - 0.175, min(y_end, yj) - 0.175, xj - x + 0.35, abs(yj - y_end) + 0.35)

    def draw_beeper(self, cr, x, b, cap):
        """Continuity symbol: a dot and three sound arcs, cap height tall."""
        cr.save()
        cr.new_path()
        cr.set_source_rgb(*C_LEGEND)
        cy = b - cap / 2
        cr.arc(x + 0.25, cy, 0.25, 0, 6.2832)
        cr.fill()
        cr.set_line_width(0.28)
        for rr in (0.6, 1.05, 1.5):
            cr.new_path()
            a = 0.95 if rr < 1.5 else 0.8
            cr.arc(x + 0.25, cy, rr, -a, a)
            cr.stroke()
        cr.restore()
        self.mark(cr, 'beeper', x, b - cap, 1.9, cap)

    def draw_diode(self, cr, x, b, cap):
        """Diode symbol ->|- (anode lead, triangle, cathode bar, lead), cap height tall."""
        cr.save()
        cr.new_path()
        cr.set_source_rgb(*C_LEGEND)
        cy = b - cap / 2
        cr.set_line_width(0.3)
        cr.move_to(x, cy)
        cr.line_to(x + 2.6, cy)
        cr.stroke()
        cr.move_to(x + 0.3, b - cap)
        cr.line_to(x + 0.3, b)
        cr.line_to(x + 2.0, cy)
        cr.close_path()
        cr.fill()
        cr.rectangle(x + 1.95, b - cap, 0.35, cap)
        cr.fill()
        cr.restore()
        self.mark(cr, 'diode', x, b - cap, 2.6, cap)

    def draw_cap(self, cr, x, b, cap):
        """Capacitor symbol -||-, 2.2 mm wide, cap height tall."""
        cr.save()
        cr.new_path()
        cr.set_source_rgb(*C_LEGEND)
        cy = b - cap / 2
        cr.set_line_width(0.3)
        cr.move_to(x, cy)
        cr.line_to(x + 0.75, cy)
        cr.move_to(x + 1.45, cy)
        cr.line_to(x + 2.2, cy)
        cr.stroke()
        cr.rectangle(x + 0.7, b - cap, 0.32, cap)
        cr.rectangle(x + 1.18, b - cap, 0.32, cap)
        cr.fill()
        cr.restore()
        self.mark(cr, 'cap', x, b - cap, 2.2, cap)

    def draw_ground(self, cr, x, cy):
        """Earth symbol, rotated with the ratings text (stem pointing left)."""
        cr.save()
        cr.new_path()
        cr.set_source_rgb(*C_LEGEND)
        cr.set_line_width(0.28)
        cr.move_to(x - 0.8, cy)
        cr.line_to(x, cy)
        for dx, hh in ((0.0, 0.8), (0.45, 0.5), (0.9, 0.22)):
            cr.move_to(x + dx, cy - hh)
            cr.line_to(x + dx, cy + hh)
        cr.stroke()
        cr.restore()
        self.mark(cr, 'ground', x - 0.8, cy - 0.8, 1.7, 1.6)

    def draw_key_icon(self, cr, code, cx, cy):
        """Touch key icons as printed on the glass: house, play/pause, auto-cycle A, 2x2 apps."""
        cr.save()
        cr.new_path()
        cr.set_source_rgb(*C_STRIP_TEXT)
        cr.set_line_width(0.38)
        cr.set_line_join(cairo.LINE_JOIN_ROUND)
        cr.set_line_cap(cairo.LINE_CAP_ROUND)
        if code == 0:  # house with a door
            cr.move_to(cx - 1.45, cy + 1.6)
            cr.line_to(cx - 1.45, cy - 0.35)
            cr.line_to(cx, cy - 1.7)
            cr.line_to(cx + 1.45, cy - 0.35)
            cr.line_to(cx + 1.45, cy + 1.6)
            cr.line_to(cx + 0.55, cy + 1.6)
            cr.line_to(cx + 0.55, cy + 0.25)
            cr.line_to(cx - 0.55, cy + 0.25)
            cr.line_to(cx - 0.55, cy + 1.6)
            cr.close_path()
            cr.stroke()
        elif code == 1:  # play / pause: outlined triangle and two bars
            cr.move_to(cx - 1.6, cy - 1.55)
            cr.line_to(cx - 1.6, cy + 1.55)
            cr.line_to(cx + 0.35, cy)
            cr.close_path()
            cr.stroke()
            cr.rectangle(cx + 0.75, cy - 1.55, 0.4, 3.1)
            cr.rectangle(cx + 1.4, cy - 1.55, 0.4, 3.1)
            cr.fill()
        elif code == 2:  # A in a circle of two chasing arrows
            r = 1.65
            for a0, a1 in ((-2.6, -0.35), (0.54, 2.79)):
                cr.new_path()
                cr.arc(cx, cy, r, a0, a1)
                cr.stroke()
                # arrowhead at a1, pointing along the (clockwise) direction of travel
                ex, ey = cx + r * cos(a1), cy + r * sin(a1)
                tx, ty = -sin(a1), cos(a1)
                nx, ny = cos(a1), sin(a1)
                cr.move_to(ex + tx * 0.75, ey + ty * 0.75)
                cr.line_to(ex + nx * 0.5, ey + ny * 0.5)
                cr.line_to(ex - nx * 0.5, ey - ny * 0.5)
                cr.close_path()
                cr.fill()
            self.text(cr, cx, cy + 0.72, 'A', 2.0, C_STRIP_TEXT, align='center', bold=True, ink=False)
        else:  # apps: three rounded squares and a circle (top right)
            q = 1.35
            for i, j in ((0, 0), (0, 1), (1, 1)):
                self.rrect(cr, cx - 1.6 + i * 1.85, cy - 1.6 + j * 1.85, q, q, 0.35)
            cr.new_sub_path()
            cr.arc(cx - 1.6 + 1.85 + q / 2, cy - 1.6 + q / 2, q / 2, 0, 6.2832)
            cr.stroke()
        cr.restore()
        self.mark(cr, 'icon%d' % code, cx - 1.75, cy - 1.75, 3.5, 3.5)


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
            if k in st:                   # lpf= / rms= appear twice: enable flag, then the data
                k += 'code'
            st[k] = v
    return st


class FrontendView(Gtk.DrawingArea):
    """Schematic of the DM40 front end: jacks -> relays -> divider / shunt / ohm network -> HY3131,
    with the route the firmware selected highlighted, then the HY3131 blocks and registers.

    All text uses the widget's own font (the same as the input cards) times `zoom`; the
    layout follows the width and the widget asks for the height its content needs."""

    def __init__(self, zoom=1.0, fit=False):
        super().__init__()
        self.st = {}
        self.zoom = zoom
        self.fit = fit            # pop-out window: zoom so the content fills the height
        self.want_h = 0
        self.ink = None               # list -> layout() records label boxes (overlap check)
        self.segs = None              # list -> layout() records wire segments (wiring check)
        self.inside_h = 0
        self.hist = {}                # remember(): what the firmware switches within a function (stable layout)
        self._ctx = None
        self.set_content_height(120)
        self.set_draw_func(self.draw)

    def update(self, st):
        self.remember(st)
        self.st = st
        self.queue_draw()

    def draw(self, area, cr, w, h):
        cr.set_source_rgb(*rgb('#1E1F22'))
        cr.rectangle(0, 0, w, h)
        cr.fill()
        z = self.zoom
        if self.fit:
            # content height at a given zoom is about proportional to zoom at width w / zoom
            z = 1.0
            for _ in range(4):
                need = self.layout(None, w / z)
                z = max(1.0, min(3.0, z * h / max(1.0, need * z)))
            # taller than the window even at zoom 1: ask for that height, the window's ScrolledWindow scrolls
            hh = int(self.layout(None, w / z) * z) + 1
            target = hh if hh > h + 1 else 100
            if target != self.want_h and (target > h + 1 or self.want_h > h + 1):
                self.want_h = target
                GLib.idle_add(lambda: self.set_content_height(self.want_h) or False)
        cr.scale(z, z)
        need = self.layout(cr, w / z)
        if not self.fit:
            hh = int(need * z) + 1
            if hh != self.want_h:
                self.want_h = hh
                GLib.idle_add(lambda: self.set_content_height(self.want_h) or False)

    # HY3131 mux option lists (emu/hy3131/ref/registers.md); the state line names the selected one
    MUX = {
        'sop1p': ['SENSE', 'FB', 'RLU', 'AGND', 'PB0', 'PB1', 'PB2', 'PB8'],
        'sop2p': ['SENSE', 'FB', 'RLU', 'AGND', 'PB0', 'PB1', 'PB2', 'PB3'],
        'sad1fp': ['SENSE', 'FB', 'RLU', 'OP1O', 'OP2O', 'VDDA', 'REFO', 'VREF',
                   'PB0', 'PB1', 'PB2', 'PB3', 'PB4', 'PB5', 'PB6', 'PB7'],
        'sad1fn': ['SENSE', 'RLU', 'VSSA', 'AGND', 'PB2', 'PB3', 'PB4', 'PB5'],
        'sad1rh': ['FB', 'REFO', 'VREF', 'PB6', 'RLU', 'VDDA', 'AGND'],
        'sad1rl': ['RLU', 'AGND', 'PB3', 'PB5', 'FB', 'VSSA', 'VREF'],
        'sad1i': ['filter', 'FB / RLU', 'TS1P / TS2N', 'TS1N / TS2P'],
        'sft1': ['100 k', '10 k', '0 Ω', 'open'],
        'sad2ip': ['OP1O', 'OP2O', 'PB4', 'PB7'],
        'sad2in': ['RLU', 'AGND', 'PB3', 'PB5'],
        'sad2rh': ['FB', 'REFO', 'VREF', 'PB6'],
        'sad2rl': ['RLU', 'AGND', 'PB3', 'PB5'],
        'sad3ip': ['OP1O', 'OP2O', 'PB4', 'PB7'],
        'sad3in': ['RLU', 'AGND', 'PB3', 'PB5'],
        'scmpi': ['SENSE', 'FB', 'OP1O', 'PB0', 'PB1', 'RLD', 'PB3', 'PB4'],
    }
    INTERNAL = {'OP1O', 'OP2O', 'REFO', 'VREF', 'VDDA', 'VSSA', 'filter', '100 k', '10 k', '0 Ω', 'open',
                'FB / RLU', 'TS1P / TS2N', 'TS1N / TS2P'}
    # SMODE<3:0> (with CMPO) -> power-bus / OP3 switches (registers.md)
    SMODE = {0: 'none', 1: 'SGND', 2: 'SVDD', 3: 'SVSS', 4: 'SVSO1', 5: 'SGND+SVSO1', 6: 'SVDD+SCP+SCN',
             7: 'SVSS+SVSO2+SCN', 8: 'none / SGND (CMPO)', 10: 'SVDD / SVSS (CMPO)', 12: 'SVSO1 / SGND+SVSO1 (CMPO)',
             14: 'SVDD+SCP+SCN / SVSS+SVSO2+SCN (CMPO)'}

    def source_band(self, st, y, w, pad, L, lw, tw, label, wire, arrow, box, opamp, dot, si, num, ink,
                    cr, on, off, hot, txt, dim, bg):
        """The test source of Ω / continuity / diode / capacitance (and RTD / NTC temperature, an Ω row):
        VREF (SFUVR) -> OP3 -> the PA switches (D / F / P) and the power bus (SMODE) -> Rref (or the RC
        oscillator's R, or a current source) -> the node Vx -> V Ω jack -> the part -> COM, with the test
        current and how AD1 / the comparator measure it. Drawn only when the row has such a source."""
        role = st.get('role', '')
        has = role in ('Ohm', 'Cont', 'Diode', 'CapOsc', 'CapRamp', 'CapDischarge') or 'rref' in st
        if not has:
            return y
        vsel = st.get('vrefsel', '?')
        try:
            sm = int(st.get('smode', '0'), 16)
        except ValueError:
            sm = 0
        closed = [self.pa_label(n, sw) for n, sw in sorted(self.pa_switches(st).items(), reverse=True)]
        # the pins that read the node (the V Ω jack's network pins the enabled blocks select)
        vg = self.context(st)['pins']             # every pin the function's states read (no jumping text)
        node_pins = [p for p in vg[0] + vg[1] if not p.startswith('PA') and p != 'FB/RLU']
        # the power bus (SMODE): with CMPO in the selection the comparator flips it between two sets (own line)
        bus_a, _, bus_b = self.SMODE.get(sm & 15, '?').partition(' / ')
        bus_b = bus_b.replace(' (CMPO)', '')
        bus = ['bus: ' + bus_a + (', RLU' if sm & 0x10 else '') + (', cap array' if sm & 0x20 else '')]
        if bus_b:
            bus.append('⇄ %s (CMPO)' % bus_b)
        y += pad * 2
        y += label(pad, y, 'test source (OP3) and the part on the V Ω jack', txt) + pad
        yc = y + L * 2.6                          # room for a three-line switch box (stable band height)
        x = pad * 2
        net = 'src'
        items_done = []
        # 1. the source (reserved widths: the blocks right of it keep their x in every function / range)
        vref_w = tw('VREF AGNDN9')
        src_w = vref_w + pad * 2 + pad * 2.5 + max(L * 2.4, tw('OP3') + L * 1.4)
        if role in ('Ohm', 'Cont', 'CapOsc'):
            b = box(x, yc, ['VREF ' + vsel, ('%s V' % st.get('vsrc', '?')) if 'vsrc' in st else
                            ('± by CMPO' if role == 'CapOsc' else '')], True, minw=vref_w)
            x = b[2] + pad * 2.5
            wire([(b[2], yc), (x, yc)], True, net=net)
            arrow(x, yc, True)
            o = opamp(x, yc, True, 'OP3', '')
            x = o[1]
        else:
            ival = num('itest')
            b = box(x, yc, ['OP3: current', si(ival, 'A') + ('  ≤ %s V' % st['compl'] if 'compl' in st else '')],
                    role != 'CapDischarge', minw=max(tw('-999.9 mA  ≤ 3.3 V'), src_w - pad * 2))
            x = b[2]
        # 2. switches: PA (D / F / P) and the power bus
        x2 = x + pad * 2.5
        wire([(x, yc), (x2, yc)], True, net=net)
        sb = box(x2, yc, [', '.join(closed) or 'PA: none'] + bus, True,
                 minw=max(tw('PA4 D F, PA3 D, PA2 D F'), tw('bus: SVSS+SVSO2+SCN, RLU, cap array'),
                          tw('⇄ SVSS+SVSO2+SCN (CMPO)')))
        x = sb[2]
        # 3. Rref / R
        rval = None
        if 'rref' in st:
            rval = 'Rref ' + si(num('rref'), 'Ω')
        elif 'rcosc' in st:
            rval = 'R ' + si(num('rcosc'), 'Ω')
        if rval:
            x3 = x + pad * 2.5
            wire([(x, yc), (x3, yc)], True, net=net)
            rw, rh = max(L * 2.4, tw('Rref 99.99 MΩ') + pad * 2, tw(rval) + pad * 2), L * 0.9
            if cr:
                cr.set_source_rgb(*bg)
                cr.rectangle(x3, yc - rh / 2, rw, rh)
                cr.fill()
                cr.set_source_rgb(*on)
                cr.set_line_width(lw)
                cr.rectangle(x3, yc - rh / 2, rw, rh)
                cr.stroke()
            if ink is not None:
                ink.append(('box Rref', x3 - 1, yc - rh / 2 - 1, x3 + rw + 1, yc + rh / 2 + 1))
            label(x3 + (rw - tw(rval)) / 2, yc - rh / 2 - L - pad * 0.8, rval, txt)
            x = x3 + rw
        else:                                       # no resistor (current source): the node stays where it is
            x1 = x + pad * 2.5 + max(L * 2.4, tw('Rref 99.99 MΩ') + pad * 2)
            wire([(x, yc), (x1, yc)], True, net=net)
            x = x1
        # 4. the node, named by the pins that read it (reserved width: the jack does not move with them)
        xn = x + pad * 4
        wire([(x, yc), (xn, yc)], True, net=net)
        dot(xn, yc, True)
        vx = st.get('vx', st.get('vramp', ''))
        nlab = 'Vx' + (' → ' + ', '.join(node_pins) if node_pins else '')
        label(xn - tw('Vx') / 2, yc - L - pad * 0.6, nlab, txt)
        # 5. jack, part, COM
        xj = xn + max(pad * 4, tw('Vx → SENSE, PB0, PB8') - tw('Vx') / 2 + pad * 2)
        wire([(xn, yc), (xj, yc)], True, net=net)
        r = L * 0.45
        if cr:
            cr.new_path()
            cr.set_source_rgb(*C_RED)
            cr.arc(xj + r, yc, r, 0, 6.3)
            cr.fill()
        if ink is not None:
            ink.append(('jack', xj - 1, yc - r - 1, xj + 2 * r + 1, yc + r + 1))
        label(xj + r - tw('V Ω') / 2, yc - r - pad * 0.8 - L, 'V Ω', txt)
        xp_ = xj + 2 * r + pad * 2.5
        wire([(xj + 2 * r, yc), (xp_, yc)], True, net='dut')
        part = {'Ohm': 'R', 'Cont': 'R', 'Diode': 'D', 'CapOsc': 'C', 'CapRamp': 'C', 'CapDischarge': 'C'}.get(role, 'R')
        pw_ = L * 2.0
        if cr:
            cr.set_source_rgb(*on)
            cr.set_line_width(lw)
            if part == 'R':
                cr.rectangle(xp_, yc - L * 0.4, pw_, L * 0.8)
                cr.stroke()
            elif part == 'D':
                cr.move_to(xp_, yc - L * 0.5)
                cr.line_to(xp_ + pw_ * 0.7, yc)
                cr.line_to(xp_, yc + L * 0.5)
                cr.close_path()
                cr.fill()
                cr.move_to(xp_ + pw_ * 0.75, yc - L * 0.5)
                cr.line_to(xp_ + pw_ * 0.75, yc + L * 0.5)
                cr.stroke()
                cr.move_to(xp_ + pw_ * 0.75, yc)
                cr.line_to(xp_ + pw_, yc)
                cr.stroke()
            else:
                cr.move_to(xp_, yc)
                cr.line_to(xp_ + pw_ * 0.4, yc)
                cr.move_to(xp_ + pw_ * 0.4, yc - L * 0.5)
                cr.line_to(xp_ + pw_ * 0.4, yc + L * 0.5)
                cr.move_to(xp_ + pw_ * 0.6, yc - L * 0.5)
                cr.line_to(xp_ + pw_ * 0.6, yc + L * 0.5)
                cr.move_to(xp_ + pw_ * 0.6, yc)
                cr.line_to(xp_ + pw_, yc)
                cr.stroke()
        if ink is not None:
            ink.append(('part', xp_ - 1, yc - L * 0.5 - 1, xp_ + pw_ + 1, yc + L * 0.5 + 1))
        pname = {'R': 'R x', 'D': 'diode', 'C': 'C x'}[part]
        label(xp_ + (pw_ - tw(pname)) / 2, yc - L * 0.5 - pad * 0.8 - L, pname, txt)
        xc_ = xp_ + pw_ + pad * 2.5
        wire([(xp_ + pw_, yc), (xc_, yc)], True, net='dut')
        if cr:
            cr.new_path()
            cr.set_source_rgb(*C_BLACK)
            cr.arc(xc_ + r, yc, r, 0, 6.3)
            cr.fill()
            cr.set_source_rgb(*txt)
            cr.arc(xc_ + r, yc, r * 0.45, 0, 6.3)
            cr.fill()
        if ink is not None:
            ink.append(('com', xc_ - 1, yc - r - 1, xc_ + 2 * r + 1, yc + r + 1))
        label(xc_ + r - tw('COM') / 2, yc - r - pad * 0.8 - L, 'COM', txt)
        # values under the chain, in fixed places (left-aligned at the blocks they belong to)
        yv = yc + (3 * L + pad * 1.2) / 2 + pad * 1.2
        vals = []
        if 'itest' in st:
            vals.append((x2, 'I test %s' % si(num('itest'), 'A')))
        if vx:
            vals.append((xn - pad * 2, 'Vx %s' % si(float(vx), 'V')))
        if 'rd_r' in st:
            vals.append((xp_, 'R %s' % si(num('rd_r'), 'Ω')))
        last = -1e9
        reserve = tw('I test -999.99 mA')            # fixed slots: the readings never move anything
        for xv, s_ in vals:
            xv = max(xv, last + pad * 2)
            label(xv, yv, s_, hot)
            last = xv + reserve
        y = yv + L + pad
        # the measurement chain from the registers (pins / op-amps / references), then what the role does
        ad1c = ''
        if st.get('ad1') == '1':
            ad1c = 'AD1: ' + self.chain(st, st.get('sad1fp', '')) + ' − ' + st.get('sad1fn', '') + \
                   ', reference ' + st.get('sad1rh', '') + ' − ' + st.get('sad1rl', '')
        cmp_in = self.context(st)['srcs']['CMP']     # every input of the range's states: steady text
        cmpc = ('comparator on ' + ' / '.join(self.chain(st, p) for p in cmp_in)) if st.get('cmp') == '1' else ''
        how = {'Ohm': 'ratiometric (R = Rref × code / (0.9 × 2²³)); ' + ad1c,
               'Cont': 'ratiometric as Ω with Rref 1 kΩ (reference order swapped: negative codes); ' + ad1c +
                       ('; the %s drives the CMP pin (beeper) below ~67 Ω' % cmpc if cmpc else ''),
               'Diode': 'OP3 as a current source; ' + ad1c +
                        (' (SDIO: the /10 divider PB0 / PB8)' if st.get('sdio') == '1' else ''),
               'CapOsc': 'RC oscillator: the %s flips VREF (CMPO) at its thresholds; f = 1 / (2 ln2 R C) on the '
                         'counter' % (cmpc or 'comparator'),
               'CapRamp': 'constant current ramp, dV/dt = I / C; ' + ad1c + '; ACPO reverses the ramp at the '
                          'R21 thresholds',
               'CapDischarge': 'discharge row: the capacitor is grounded'}.get(role, ad1c)
        if how:
            y += label(pad * 2, y, how, dim, w - pad * 4) + pad
        return y

    @staticmethod
    def chain(st, pin):
        """a block input as the registers route it: 'PB0 → OP1 ×1 → OP1O' for an op-amp output"""
        for o in ('1', '2'):
            if pin == 'OP%sO' % o and st.get('op' + o) == '1':
                return '%s → OP%s %s → %s' % (st.get('sop%sp' % o, '?'), o, '×10' if st.get('ops' + o) == '1' else '×1', pin)
        if pin == 'PB4' and st.get('op1') == '1':
            return '%s → OP1 → OP1O → AC coupling → PB4' % st.get('sop1p', '?')
        return pin

    # Board facts (pin -> jack net). The DM40 netlist is not known (emu/hy3131/README.md); what is used:
    # the PA<9:0> switch network is the V Ω jack's resistor network (datasheet 12.2: the divider in V,
    # the reference resistors / OP3 source in Ω), SENSE and PB5 its V-path nodes (DC V reads SENSE − PB5),
    # PB0 / PB8 (SDIO /10 divider), RLU / RLD and FB the Ω / mV network's, PB2 the A shunt, PB1 the mA / uA
    # shunt, PB3 and AGND COM. PB4 is the AC-coupled OP1 output, so it carries whatever OP1's input does.
    # Which of them a frame shows is decided by the registers only (muxes of the enabled blocks, PA switches).
    NET_VA = ('SENSE', 'PB5')                          # V jack via the divider (PA relay V/A)
    NET_OHM = ('PB0', 'PB8', 'SENSE', 'RLU', 'RLD', 'FB')  # V jack via the Ω / mV network (PA relay Ω/mV)
    NET_COM = ('AGND', 'PB3')
    NET_A = ('PB2', 'PB1')
    # divider taps with SS closed (the stock / fw-rs rows) -> the ratio at SENSE
    TAPS = {(7,): '/10', (6,): '/100', (5,): '/1000', (4, 5): '/10000'}

    @staticmethod
    def pa_switches(st):
        """closed PA switches from the state line's pasw ("9S,5F,4D") -> {pin number: 'PS'-style letters}"""
        closed = {}
        for part in st.get('pasw', '-').split(','):
            if part and part[0].isdigit():
                n = int(part.rstrip('PDFS'))
                closed[n] = part[len(str(n)):]
        return closed

    @staticmethod
    def pa_label(n, sw):
        return 'PA%d %s' % (n, ' '.join(sw))

    @classmethod
    def divider_ratio(cls, st):
        """V divider ratio from the taps the row connects to SENSE (SS closed), '' when not a known set"""
        taps = tuple(sorted(n for n, sw in cls.pa_switches(st).items() if 'S' in sw and n in (4, 5, 6, 7)))
        return cls.TAPS.get(taps, '')

    @classmethod
    def used_pins(cls, st):
        """{pin: [consumers]} the enabled blocks select: OP1 / OP2 + inputs, AD1 (input by SAD1I, reference),
        AD2 / AD3 inputs and references, the comparator input (ENCMP), SDIO (PB0 / PB8)"""
        def on(k):
            return st.get(k) == '1'
        used = {}

        def use(pin, who):
            if pin and pin != '-':
                used.setdefault(pin, []).append(who)
        if on('op1'):
            use(st.get('sop1p'), 'OP1')
        if on('op2'):
            use(st.get('sop2p'), 'OP2')
        if on('ad1'):
            si = st.get('sad1i', '0')
            if si == '0':
                use(st.get('sad1fp'), 'AD1')
                use(st.get('sad1fn'), 'AD1-')
            elif si == '1':
                use('FB', 'AD1')
                use('RLU', 'AD1-')
            use(st.get('sad1rh'), 'AD1 ref')
            use(st.get('sad1rl'), 'AD1 ref')
        if on('ad2'):
            for k in ('sad2ip', 'sad2in', 'sad2rh', 'sad2rl'):
                use(st.get(k), 'AD2')
        if on('ad3'):
            for k in ('sad3ip', 'sad3in'):
                use(st.get(k), 'AD3')
        if on('cmp'):
            use(st.get('scmpi'), 'CMP')
        if on('sdio'):
            use('PB0', 'SDIO')
            use('PB8', 'SDIO')
        return used

    @classmethod
    def jack_pins(cls, st):
        """HY3131 pins on each jack wire, from the register state only -> (V path, Ω/mV network, COM, A)
        lists of labels, one pin per label: the pins the enabled blocks select (used_pins) that the
        board facts above put on that jack, and every closed PA switch ('PA6 P S': P power bus, D OP3
        output, F FB, S SENSE) on the V Ω jack's network. The V Ω pins go to the V path or the Ω / mV
        network by the PA relay (pa). FB and RLU both in the AD1 reference: one label 'FB/RLU'."""
        used = cls.used_pins(st)
        va = st.get('pa', '0') == '0'
        vnet = cls.NET_VA if va else cls.NET_OHM
        net_of = {}
        for p in vnet:
            net_of[p] = 'v'
        for p in cls.NET_COM:
            net_of[p] = 'c'
        for p in cls.NET_A:
            net_of[p] = 'a'
        if st.get('op1') == '1' and st.get('sop1p') in net_of:   # PB4 = OP1O through the AC coupling
            net_of['PB4'] = net_of[st.get('sop1p')]
        g = {'v': [], 'c': ['AGND'], 'a': []}
        ref = {st.get('sad1rh'), st.get('sad1rl')} == {'FB', 'RLU'} and st.get('ad1') == '1'
        for p in ('SENSE', 'PB0', 'PB8', 'RLU', 'RLD', 'FB', 'PB4', 'PB5', 'PB3', 'PB2', 'PB1'):
            if p not in used or p not in net_of:
                continue
            if ref and p in ('FB', 'RLU') and all(w == 'AD1 ref' for w in used[p]):
                if 'FB/RLU' not in g['v']:
                    g['v'].append('FB/RLU')
                continue
            g[net_of[p]].append(p)
        g['v'] += [cls.pa_label(n, sw) for n, sw in sorted(cls.pa_switches(st).items(), reverse=True)]
        return (g['v'], [], g['c'], g['a']) if va else ([], g['v'], g['c'], g['a'])

    # ---------- inputs the firmware switches inside one function / range ----------
    # The drawing holds its geometry per function / range: every pin, wire and line any of these states
    # can need is reserved, so a toggle never adds or removes a row (the owner: "скачет, некрасиво").
    # fw-rs time-shares inputs (fw-rs/src/dmm.rs, sentinel.rs; power-meter-and-misconnect.md 4.4):
    #  - DC V 6 V: the comparator alternates PB1 (A-jack sentinel) / SENSE (AC? hint), one 100 ms window each;
    #  - AC V: AD1 (peak hold) alternates PB1 - PB3 (A-jack) / SENSE - PB5 (DC? hint), 100 ms windows;
    #  - DC V 6 V / DC mV: after a comparator hit the A-PROT probe (OP2 on PB2, AD2 = OP2O - PB3, peak hold
    #    on AD2) joins the row, and leaves again (sentinel::probe_row);
    #  - frequency: the counter source alternates every 350 ms between the comparator (PB4) and the CNT pin;
    #  - power meter: the RMS statistic cycles (SVXI / SDO23, method B), the counter's comparator moves
    #    between the V tap (SENSE) and the current (PB4) by level;
    #  - capacitance: the first counter read moves the comparator from PB0 (the row) to OP1O (R20 0x50);
    #  - thermocouple: AD1 cycles TS1N / TS2P, TS1P / TS2N and its input (SAD1I 3 / 2 / 0), ~0.75 s each;
    #  - AC+DC: whole AC V / DC V rows alternate (~1 s each);
    #  - ENPKH off for a few ms at each peak-hold restart (continuity, diode, the probe).
    # Known ones are predicted from their register signature (model_variants: the stock rows never show
    # it), the rest are learnt (remember: every state seen since the function / range was entered).
    HIST_N = 8           # state updates looked back for an alternation (the line changes <= ~10 times / s)
    PHASE_N = 40         # updates of one AC+DC phase at most (two rows alternating = one drawing)
    ALT_GROUPS = {'cmp': ('cmp', 'scmpi'), 'ad1': ('ad1', 'sad1i', 'sad1fp', 'sad1fn')}
    TRACK = ('cmp', 'scmpi', 'ad1', 'sad1i', 'sad1fp', 'sad1fn', 'sdio', 'op1', 'sop1p', 'ops1', 'op2', 'sop2p',
             'ops2', 'ad2', 'sad2ip', 'sad2in', 'sad2rh', 'sad2rl', 'ad3', 'sad3ip', 'sad3in', 'pkh', 'pkhsel',
             'pasw', 'sad1rh', 'sad1rl', 'svxi', 'sdo23')
    PROBE = {'op2': '1', 'sop2p': 'PB2', 'ops2': '0', 'ad2': '1', 'sad2ip': 'OP2O', 'sad2in': 'PB3',
             'sad2rh': 'PB6', 'sad2rl': 'AGND', 'pkh': '1', 'pkhsel': '0'}
    CONSUMERS = ('AD1', 'OP1', 'AD2', 'OP2', 'AD3', 'CMP')
    EN = {'AD1': 'ad1', 'AD2': 'ad2', 'AD3': 'ad3', 'CMP': 'cmp', 'OP1': 'op1', 'OP2': 'op2'}

    @staticmethod
    def fkey(st):
        """the function / range the layout is held for"""
        return tuple(st.get(k, '') for k in ('role', 'mode', 'range', 'pa', 'shunt', 'powered'))

    @staticmethod
    def src_of(st, name):
        """the input a block reads in state st (AD1 by SAD1I: TS / FB / its mux)"""
        if name == 'AD1':
            si_ = st.get('sad1i', '0')
            return 'TS' if si_ in ('2', '3') else 'FB' if si_ == '1' else st.get('sad1fp', '')
        return st.get({'AD2': 'sad2ip', 'AD3': 'sad3ip', 'CMP': 'scmpi', 'OP1': 'sop1p', 'OP2': 'sop2p'}[name], '')

    @classmethod
    def gval(cls, st, g):
        """an alternation group's value: its fields, or ('0',) with the block off"""
        f = cls.ALT_GROUPS[g]
        return tuple(st.get(k, '') for k in f) if st.get(f[0]) == '1' else ('0',)

    @classmethod
    def gapply(cls, st, g, val):
        f = cls.ALT_GROUPS[g]
        return dict(st, **dict(zip(f, val))) if val[0] == '1' else dict(st, **{f[0]: '0'})

    @classmethod
    def gtext(cls, st, g, val):
        """how a group value reads in the drawing: 'PB1', 'off', 'PB1 − PB3', 'TS'"""
        if val[0] != '1':
            return 'off'
        s = cls.gapply(st, g, val)
        if g == 'cmp':
            return s.get('scmpi', '')
        a = cls.src_of(s, 'AD1')
        return a + ((' − ' + s.get('sad1fn', '')) if a not in ('TS', 'FB') else '')

    PIN_ORDER = ('AGND', 'SENSE', 'PB0', 'PB8', 'RLU', 'RLD', 'FB', 'FB/RLU', 'PB4', 'PB5', 'PB3', 'PB2', 'PB1')

    @classmethod
    def pin_rank(cls, p):
        """a fixed order of pins / inputs (jack pin lists, a block's inputs): nothing swaps between states"""
        if p in cls.PIN_ORDER:
            return (cls.PIN_ORDER.index(p), p)
        if p.startswith('PA') and p[2:3].isdigit():
            return (50 - int(p.split()[0][2:]), p)
        return (99, p)

    @classmethod
    def alt_sort(cls, st, g, vals):
        """distinct group values in a fixed order (blocks on first, the A-jack sentinel first): steady text"""
        return sorted(dict.fromkeys(vals), key=lambda v: (v[0] != '1', cls.gtext(st, g, v) not in ('PB1', 'PB1 − PB3'),
                                                          cls.gtext(st, g, v)))

    @classmethod
    def model_variants(cls, st):
        """the states fw-rs switches this row between, from its register signature (header comment)"""
        out = []
        role, rng = st.get('role'), st.get('range')
        cmp_s = st.get('scmpi') if st.get('cmp') == '1' else None
        if role in ('DcV', 'DcMv') and cmp_s in ('PB1', 'SENSE'):          # the CmpA sentinel row
            if role == 'DcV' and rng == '0':
                out.append(dict(st, scmpi='SENSE' if cmp_s == 'PB1' else 'PB1'))
            probe = all(st.get(k) == v for k, v in cls.PROBE.items() if k not in ('pkh', 'ops2'))
            if probe:
                out.append(dict(st, op2='0', ad2='0', pkh='0'))
            elif st.get('op2') != '1' and st.get('ad2') != '1':
                out.append(dict(st, **cls.PROBE))
        a1 = (st.get('sad1fp'), st.get('sad1fn'))
        if role == 'AcV' and st.get('ad1') == '1' and st.get('pkhsel') == '1' and a1 in (('PB1', 'PB3'), ('SENSE', 'PB5')):
            o = ('SENSE', 'PB5') if a1 == ('PB1', 'PB3') else ('PB1', 'PB3')
            out.append(dict(st, sad1fp=o[0], sad1fn=o[1]))
        if role == 'Freq' and (cmp_s == 'PB4' or st.get('cmp') != '1'):     # counter: CMP (0xF0) / CNT (0x08)
            out.append(dict(st, cmp='0', scmpi='SENSE') if cmp_s == 'PB4' else dict(st, cmp='1', scmpi='PB4'))
        if role == 'Power' and cmp_s in ('SENSE', 'PB4'):          # pw_cmp_select: V tap / current by level
            out.append(dict(st, scmpi='PB4' if cmp_s == 'SENSE' else 'SENSE'))
        if role == 'CapOsc' and cmp_s in ('PB0', 'OP1O'):        # the first counter read moves it (0x50)
            out.append(dict(st, scmpi='OP1O' if cmp_s == 'PB0' else 'PB0'))
        if role == 'Temp' and st.get('ad1') == '1':
            out += [dict(st, sad1i=s) for s in '023' if st.get('sad1i', '0') != s]
        return out

    def remember(self, st):
        """state history: the last few function / range segments (AC+DC alternates two), per segment
        the recent states (alternations) and every distinct register selection seen (reserved)"""
        h = self.hist
        k = self.fkey(st)
        segs = h.setdefault('segs', [])
        if segs and segs[-1][0] == k:
            segs[-1][1] += 1
            segs[-1][2] = st
        else:
            segs.append([k, 1, st])
            del segs[:-4]
        keep = {s[0] for s in segs}
        for d in ('recent', 'seen'):
            h[d] = {k_: v for k_, v in h.get(d, {}).items() if k_ in keep}
        sig = tuple(st.get(t, '') for t in self.TRACK)
        rec = h['recent'].setdefault(k, [])
        rec.append(sig)
        del rec[:-self.HIST_N]
        seen = h['seen'].setdefault(k, {})
        if len(seen) < 24:
            seen.setdefault(sig, None)
        self._ctx = None

    def context(self, st):
        """-> dict: variants (the states the geometry holds), alt {group: values} (alternating now),
        pins (union jack pin lists), pin_st {pin: on / shared / off}, srcs {block: [inputs]},
        src_st {(block, input): status}, act {block: drawn active}, phases (AC+DC: the other row's role)"""
        if self._ctx is not None and self._ctx[0] is st:
            return self._ctx[1]
        k = self.fkey(st)
        h = self.hist
        segs = h.get('segs', [])
        known = bool(segs) and segs[-1][0] == k and segs[-1][2] is st
        seen = h.get('seen', {}) if known else {}
        recent = h.get('recent', {}).get(k, []) if known else []

        def expand(s, key):
            vs = [s] + self.model_variants(s)
            for sig in seen.get(key, {}):
                v = dict(s, **dict(zip(self.TRACK, sig)))
                vs += [v] + self.model_variants(v)
            return vs
        variants = expand(st, k)
        # AC+DC: the row alternates with another function's (A B A): the drawing holds both
        other = None
        if known and len(segs) >= 3 and segs[-3][0] == k and segs[-2][1] <= self.PHASE_N:
            other = segs[-2][2]
            variants += expand(other, segs[-2][0])
        # alternations within this function / range: >= 2 values and >= 2 switches in the last updates
        alt = {}
        for g, f in self.ALT_GROUPS.items():
            idx = [self.TRACK.index(x) for x in f]
            seq = [tuple(s_[i] for i in idx) if s_[idx[0]] == '1' else ('0',) for s_ in recent]
            vals = list(dict.fromkeys(seq))
            if len(vals) >= 2 and sum(1 for a, b in zip(seq, seq[1:]) if a != b) >= 2:
                alt[g] = self.alt_sort(st, g, vals)
        alt_states = [st] + [self.gapply(st, g, v) for g, vs in alt.items() for v in vs]
        # jack pins: union over the variants, status from the alternation states
        pins = [[], [], [], []]
        for v in variants:
            for i, lst in enumerate(self.jack_pins(v)):
                for p in lst:
                    if p not in pins[i]:
                        pins[i].append(p)
        sets = [set(p for lst in self.jack_pins(a) for p in lst) for a in alt_states]
        on_p, any_p = set.intersection(*sets), set.union(*sets)
        pin_st = {p: 'on' if p in on_p else 'shared' if p in any_p else 'off' for lst in pins for p in lst}
        # block inputs: every input a variant reads with the block on (else the current, inactive)
        srcs, src_st, act = {}, {}, {}
        for n in self.CONSUMERS:
            lst = []
            for v in variants:
                if v.get(self.EN[n]) == '1':
                    s_ = self.src_of(v, n)
                    if s_ and s_ not in lst:
                        lst.append(s_)
            srcs[n] = sorted(lst, key=self.pin_rank) or [self.src_of(st, n)]
            now = [self.src_of(a, n) if a.get(self.EN[n]) == '1' else None for a in alt_states]
            act[n] = any(x is not None for x in now)
            for s_ in srcs[n]:
                src_st[(n, s_)] = ('on' if all(x == s_ for x in now) else 'shared') if s_ in now else 'off'
        ip = self.TRACK.index('pkh')
        r = dict(variants=variants, alt=alt, pins=pins, pin_st=pin_st, srcs=srcs, src_st=src_st, act=act,
                 other=other, pkh_recent=any(s_[ip] == '1' for s_ in recent[-3:]))
        self._ctx = (st, r)
        return r

    # notes for the alternations (group, the values as drawn) -> text; the first match wins
    ALT_NOTES = (
        ('cmp', {'PB1', 'SENSE'}, 'alternates every 100 ms (A-jack / AC? hint)'),
        ('cmp', {'PB4', 'off'}, 'on / off every 350 ms (counter: comparator ⇄ CNT pin)'),
        ('ad1', {'PB1 − PB3', 'SENSE − PB5'},
         'AD1 alternates every 100 ms: PB1 − PB3 (A-jack peak hold) ⇄ SENSE − PB5 (DC? hint)'),
    )

    def alt_note(self, st, g, vals):
        txt = [self.gtext(st, g, v) for v in vals]
        for g_, s_, note in self.ALT_NOTES:
            if g_ == g and set(txt) == s_:
                return note
        if g == 'ad1' and 'TS' in txt:
            return 'AD1 cycles with the thermocouple phases (~0.75 s each): %s' % ' ⇄ '.join(dict.fromkeys(txt))
        return '%s switched by the firmware: %s' % ({'cmp': 'comparator', 'ad1': 'AD1'}[g], ' ⇄ '.join(dict.fromkeys(txt)))

    def layout(self, cr, w):
        """Signal-flow block diagram, drawn (cr) or only measured (cr None) at width w; returns the height.

        Left to right: jacks -> PA relay / shunt -> divider / Ω-mV network -> the HY3131 pins the row
        selects -> OP1 / OP2 (their + inputs) -> AD1 (24-bit ΣΔ) / AD2 / AD3 (fast) -> per channel: HPF ->
        the RMS block's multiplier (X·X, X·Y, Y·Y by SVXI / SDO23) -> mean; LPF (mean of X or Y by
        SDO23); peak hold; comparator -> counter; with the live results and what they mean at the jacks.
        Active paths blue, inactive grey. Narrow widths put the board part above the chip part."""
        st = self.st
        on, off, hot, txt = rgb('#3584E4'), rgb('#6E6E6E'), rgb('#F3B51E'), rgb('#E8E8E8')
        dim, chipc, bg = rgb('#9A9A9A'), rgb('#2A2C31'), rgb('#1E1F22')
        pl = self.create_pango_layout('Ag')
        L = pl.get_pixel_size()[1]
        pad = L * 0.4
        ink = self.ink if cr is not None else None
        lw = max(1.5, L / 9)

        def tw(s):
            pl.set_text(s, -1)
            return pl.get_pixel_size()[0]

        def label(x, y, s, color=txt, width=None, mark=True, draw=True):
            pl.set_text(s, -1)
            pl.set_width(int(width * Pango.SCALE) if width else -1)
            pl.set_wrap(Pango.WrapMode.WORD_CHAR)
            w_, h_ = pl.get_pixel_size()
            if not draw:                            # measure only (a line held for text that may come)
                pl.set_width(-1)
                return h_
            if cr:
                cr.set_source_rgb(*color)
                cr.move_to(x, y)
                PangoCairo.show_layout(cr, pl)
            if ink is not None and mark and s.strip():
                ink.append((s, x, y, x + w_, y + h_))
            pl.set_width(-1)
            return h_

        segs = self.segs if cr is not None and self.segs is not None else None
        wire_n = [0]

        def wire(pts, active, width=None, net=None, record=True, dash=False):
            """orthogonal polyline; segments recorded (net id) for the wiring check; dash: an input the
            firmware time-shares (drawn the same in either phase: nothing blinks)"""
            q = [pts[0]]
            for p_ in pts[1:]:
                if abs(p_[0] - q[-1][0]) > 0.01 or abs(p_[1] - q[-1][1]) > 0.01:
                    q.append(p_)
            if len(q) < 2:
                return
            wire_n[0] += 1
            if segs is not None and record:
                for a_, b_ in zip(q, q[1:]):
                    segs.append((net if net is not None else 'w%d' % wire_n[0], a_[0], a_[1], b_[0], b_[1]))
            if cr:
                cr.set_source_rgb(*(on if active else off))
                cr.set_line_width(width or lw)
                if dash:
                    cr.set_dash([lw * 2.5, lw * 1.8])
                cr.move_to(*q[0])
                for p_ in q[1:]:
                    cr.line_to(*p_)
                cr.stroke()
                cr.set_dash([])

        def dot(x_, y_, active):
            if cr:
                cr.new_path()
                cr.set_source_rgb(*(on if active else off))
                cr.arc(x_, y_, lw * 1.6, 0, 6.3)
                cr.fill()

        def arrow(x, y, active):
            """arrowhead pointing right, tip at x"""
            if cr:
                a = L * 0.3
                cr.set_source_rgb(*(on if active else off))
                cr.move_to(x, y)
                cr.line_to(x - a, y - a * 0.6)
                cr.line_to(x - a, y + a * 0.6)
                cr.close_path()
                cr.fill()

        def box(x, yc, lines, active, minw=0, fill=None):
            """rectangle with centred text lines; -> (x0, y0, x1, y1)"""
            bw = max([tw(s) for s in lines] + [minw]) + pad * 2
            bh = len(lines) * L + pad * 1.2
            y0 = yc - bh / 2
            if cr:
                cr.set_source_rgb(*(fill or bg))
                cr.rectangle(x, y0, bw, bh)
                cr.fill()
                cr.set_source_rgb(*(on if active else off))
                cr.set_line_width(lw)
                cr.rectangle(x, y0, bw, bh)
                cr.stroke()
            for i, s in enumerate(lines):
                label(x + (bw - tw(s)) / 2, y0 + pad * 0.6 + i * L, s, (txt if i == 0 else dim) if active else off,
                      mark=False)
            if ink is not None:
                ink.append(('box ' + lines[0], x - 1, y0 - 1, x + bw + 1, y0 + bh + 1))
            return x, y0, x + bw, y0 + bh

        def box_w(lines, minw=0):
            return max([tw(s) for s in lines] + [minw]) + pad * 2

        def opamp(x, yc, active, name, gain, hold=False):
            """triangle opamp symbol (width ~2.2 L), name inside, gain under it; -> (x_in, x_out)"""
            h_ = L * 2.0
            if gain:
                name = name + ' ' + gain
            # hold: as wide as the widest name (a gain that appears when the probe switches OP2 on moves nothing)
            w_ = max(L * 2.4, tw(name) + L * 1.4, tw('OP1 ×10') + L * 1.4 if hold else 0)
            if cr:
                cr.set_source_rgb(*bg)
                cr.move_to(x, yc - h_ / 2)
                cr.line_to(x + w_, yc)
                cr.line_to(x, yc + h_ / 2)
                cr.close_path()
                cr.fill_preserve()
                cr.set_source_rgb(*(on if active else off))
                cr.set_line_width(lw)
                cr.stroke()
            label(x + pad * 0.6, yc - L / 2, name, txt if active else off, mark=False)
            if ink is not None:
                ink.append(('op ' + name, x - 1, yc - h_ / 2 - 1, x + w_ + 1, yc + h_ / 2 + 1))
            return x, x + w_

        def flag(k):
            return st.get(k) == '1'

        def num(k, default=0.0):
            try:
                return float(st.get(k, default))
            except ValueError:
                return default

        def si(v, unit):
            """engineering format: 1.234 V, 12.3 mA, 4.70 kΩ"""
            a = abs(v)
            if a == 0:
                return '0 %s' % unit
            for f, p in ((1e9, 'G'), (1e6, 'M'), (1e3, 'k'), (1, ''), (1e-3, 'm'), (1e-6, 'µ'), (1e-9, 'n')):
                if a >= f * 0.9995 or f == 1e-9:
                    return '%.4g %s%s' % (v / f, p, unit)
            return '%.4g %s' % (v, unit)

        if cr:
            cr.set_line_width(lw)
        if not st:
            return label(pad, pad, 'waiting for the HY3131 state …', txt, w - 2 * pad) + 2 * pad

        role = st.get('role', 'None')
        pa = st.get('pa', '0')
        va = pa == '0'
        rng = st.get('range', '0')
        shunt = st.get('shunt', 'uA')
        power = role == 'Power'
        # The layout depends on the configuration only (function, range, relays, muxes, switches), never on
        # readings or on the counter's ENCTR toggling: the comparator counts as used by ENCMP alone.
        shunts = {'A': '10 mΩ', 'mA': '0.5 Ω', 'uA': '47.5 Ω'}
        # what the firmware switches inside this function / range: the geometry holds all of it (context)
        ctx = self.context(st)
        pin_v, pin_n, pin_c, pin_a = ctx['pins']   # every pin a state of this function / range can use
        pin_st = ctx['pin_st']

        def split(lines):
            return [p.strip() for s in lines for p in s.split(',') if p.strip()]
        pv, pn, pc, pa_ = split(pin_v), split(pin_n), split(pin_c), split(pin_a)

        y = pad
        title = 'HY3131  %s  mode %s  range %s%s' % (role, st.get('mode', '-1'), rng,
                                                    '' if st.get('powered', '1') == '1' else '   (unpowered: PE2 off)')
        y += label(pad, y, title, txt, w - 2 * pad) + pad

        # ---------- chip part: rows and blocks ----------
        RH = L * 2.6
        stat = 2 if flag('sdo23') else 1 if flag('svxi') else 0          # RMS: X·X, X·Y, Y·Y
        mstat = ['X·X', 'X·Y', 'Y·Y'][stat]
        lpf_y = flag('sdo23')
        alt = ctx['alt']
        srcs, src_st = ctx['srcs'], ctx['src_st']
        act = ctx['act']                           # a time-shared block counts as on in either phase
        ad1, ad2, ad3, op1, op2 = act['AD1'], act['AD2'], act['AD3'], act['OP1'], act['OP2']
        cmp_used = act['CMP']
        # the AD2 stream (Wave view): not the A-PROT probe (its row signature, not ENPKH: that blinks)
        probe = all(st.get(k) == v for k, v in self.PROBE.items() if k not in ('pkh', 'ops2'))
        stream = flag('ad2') and not flag('rms') and not flag('lpf') and not probe
        cmp_s = ' ⇄ '.join(self.gtext(st, 'cmp', v) for v in alt['cmp']) if 'cmp' in alt else st.get('scmpi', '')
        ad1_n, ad1_sp = st.get('sad1fn', ''), '  '
        if 'ad1' in alt:                            # AC V: '− PB3⇄5 ref …' (fits the reserved converter width)
            ns = list(dict.fromkeys(self.gapply(st, 'ad1', v).get('sad1fn', '') for v in alt['ad1'] if v[0] == '1'))
            ad1_n = ('PB' + '⇄'.join(n_[2:] for n_ in ns)) if all(n_.startswith('PB') for n_ in ns) else '⇄'.join(ns)
            ad1_sp = ' ' if len(ns) > 1 else '  '

        # text of the converter boxes
        b_ad1 = ['AD1  24-bit ΣΔ', 'OSR %s%s  IG %s' % (st.get('osr1', ''), ' chop' if flag('chop') else '',
                                                      st.get('ad1ig', '')),
                 '− %s%sref %s/%s' % (ad1_n, ad1_sp, st.get('sad1rh', ''), st.get('sad1rl', ''))]
        b_ad2 = ['AD2  fast', 'OSR %s  IG %s' % (st.get('osr2', ''), st.get('ad2ig', '')),
                 '− %s  %s S/s' % (st.get('sad2in', ''), si(num('ad2rate'), '').strip())]
        b_ad3 = ['AD3  fast', 'OSR %s  IG %s' % (st.get('osr2', ''), st.get('ad3ig', '')),
                 '− %s' % st.get('sad3in', '')]
        b_cmp = ['CMP', cmp_s]
        cmp_w = box_w(['PB1 ⇄ SENSE', 'PB4 ⇄ off'])      # reserved: the counter right of it never moves
        # column widths are reserved for the widest text any row can produce: no column moves between
        # functions / ranges (the owner: nothing jumps horizontally between frames)
        # (the widest texts of the stock / fw-rs rows; an unusual row may widen them, never the usual ones)
        conv_w = max([box_w(b) for b in (b_ad1, b_ad2, b_ad3)] + [box_w([
            'OSR 32768 chop  IG 0.9', '− VSSA  ref REFO/AGND', '− AGND  76.8 k S/s', '− PB3⇄5 ref PB6/AGND'])])

        # results
        res1 = []
        for k, u in (('rd_vdc', 'V'), ('rd_idc', 'A'), ('rd_r', 'Ω'), ('rd_ad1idc', 'A')):
            if k in st and ad1 and not (power and k == 'rd_vdc') and not (power and k == 'rd_idc'):
                res1.append(('I dc' if k == 'rd_ad1idc' else {'V': 'V dc', 'A': 'I dc', 'Ω': 'R'}[u]) + ' ' + si(num(k), u))
        res1.append('code %s' % st.get('ad1code', ''))
        rms_res, rms_unit = None, ''
        for k, u, n in (('rd_p', 'W', 'P'), ('rd_vrms', 'V', 'V rms'), ('rd_irms', 'A', 'I rms')):
            if k in st:
                rms_res = '%s %s' % (n, si(num(k), u))
        lpf_res = None
        if power:
            k = 'rd_vdc' if lpf_y else 'rd_idc'
            if k in st:
                lpf_res = '%s %s' % ('V dc' if lpf_y else 'I dc', si(num(k), 'V' if lpf_y else 'A'))
        if lpf_res is None:
            lpf_res = 'mean %s' % st.get('lpfcode', '0')
        rmscode = st.get('rmscode', '0')

        # ---------- board part geometry ----------
        r = L * 0.45
        name_w = max(tw(n) for n in ('V Ω', 'COM', 'A'))
        relay_lines = ['PA: V/A' if va else 'PA: Ω/mV']
        net_v = ['divider ' + self.divider_ratio(st)]
        net_n = ['Ω/mV net']
        sh_lines = ['shunt %s' % shunt, shunts.get(shunt, '')]
        relay_w = box_w(['PA: Ω/mV'])
        net_w = max(box_w(net_v), box_w(net_n))
        sh_w = max(box_w(['shunt %s' % k, v]) for k, v in shunts.items())
        net_w = max(net_w, box_w(['divider /10000']))
        pins_w = max([tw(p) for p in pv + pn + pc + pa_] + [tw(p) for p in ('SENSE', 'FB/RLU', 'PA8 D F')]) + pad
        board_w = pad + 2 * r + pad + name_w + pad * 2 + relay_w + pad * 3 + max(net_w, sh_w) + pad * 3 + pins_w

        # ---------- chip part geometry (x relative to x0) ----------
        mul_r = L * 0.75
        hpf_w = box_w(['HPF'])
        rms_lines = ['RMS block', 'Σ %s / N' % mstat]
        rms_w = box_w(rms_lines)
        lpf_lines = ['LPF (mean)', '%s, OSR %s' % ('Y' if lpf_y else 'X', st.get('osr4', ''))]
        lpf_w = box_w(lpf_lines)
        # reserved, not measured: the layout must not move when the readings change
        res_w = max(tw('V rms -999.9 mV'), tw('code -8388608'), tw('RMS 68719476735'), tw('stream → MCU 76.8 k S/s')) + pad
        step = max(9.0, pad * 1.4)                    # routing track pitch (>= 8 px between wires)
        netsA_n = len({v for k in srcs for v in srcs[k] if v in pv + pn + pc + pa_})
        laneA = (max(netsA_n, 4) + 1) * step + pad
        op_w = max(L * 2.4, tw('OP1 ×10') + L * 1.4)
        stubs = [v for k in srcs for v in srcs[k] if v and v not in ('OP1O', 'OP2O') and v not in (pv + pn + pc + pa_)]
        stub_names = set(self.MUX['sad1fp'] + self.MUX['sad2ip'] + self.MUX['scmpi'] + self.MUX['sop1p']) - {'OP1O', 'OP2O'}
        laneB = max(3 * step + pad, max([tw(v_) for v_ in stubs + list(stub_names) + ['TS']]) + pad * 3)
        chip_w = (pins_w + laneA + op_w + laneB + conv_w + pad * 4 + hpf_w + pad * 3 + mul_r * 2 + pad * 3 + rms_w
                  + pad * 2 + res_w + pad)
        side = board_w + chip_w + pad * 2 <= w

        # ---------- rows ----------
        y_ad1 = y + RH * 0.6
        y_op1 = y_ad1 + RH
        y_ad2 = y_op1 + RH
        y_op2 = y_ad2 + RH
        y_ad3 = y_op2 + RH
        y_lpf = y_ad3 + RH
        y_cmp = y_lpf + RH
        chip_bottom = y_cmp + RH * 0.7 + L

        # board rows (side by side: aligned with the chip rows; stacked: own band above)
        row_of = {'AD1': 0, 'OP1': 1, 'AD2': 2, 'OP2': 3, 'AD3': 4, 'CMP': 6}

        def feeds(p):
            return min([row_of[k] for k in srcs if p in srcs[k]] + [99])
        def order(p):                               # by the block it feeds, then a fixed pin order (no swaps)
            return (feeds(p), self.pin_rank(p))
        pv, pn, pc, pa_ = (sorted(g_, key=order) for g_ in (pv, pn, pc, pa_))

        def pin_rows(y_top):
            """y of every pin, grouped by jack row: V pins, network pins, COM pins, A pins"""
            ys = {}
            yy = y_top + (0 if va else RH)          # Ω / mV: the V jack row above the network's pins
            groups = [('v', pv), ('c', pc), ('a', pa_)] if va else [('n', pn), ('c', pc), ('a', pa_)]
            for g, pins in groups:
                lst = pins or ['–']
                ys[g] = []
                for p in lst:
                    ys[g].append((p, yy))
                    yy += L + pad * 0.8
                yy += pad * 1.5
            return ys, yy

        bx0 = pad
        x_pin = bx0 + board_w - pins_w              # board: chip edge = where the pin names start
        if side:
            board_top = y_op1 - L
            cx0 = x_pin
        else:
            board_top = y + L * 0.6
        pys, board_bottom = pin_rows(board_top)
        if not side:
            # chip below the board band, with its own pin column on its left edge
            shift = board_bottom + L * 2.5 - y
            y_ad1 += shift; y_op1 += shift; y_ad2 += shift; y_op2 += shift
            y_ad3 += shift; y_lpf += shift; y_cmp += shift; chip_bottom += shift
            cx0 = pad * 2
        jack_y = {g: (sum(p[1] for p in pys[g]) / len(pys[g]) + L / 2) for g in pys}
        jack_y.setdefault('v', jack_y.get('n', board_top))
        jack_y.setdefault('n', jack_y['v'])
        pys.setdefault('v', [])
        pys.setdefault('n', [])

        # ---------- draw: chip background ----------
        ad1_half = (3 * L + pad * 1.2) / 2
        chip_top = min(board_top - pad, y_ad1 - ad1_half - pad) if side else (y_ad1 - ad1_half - pad)
        chip_bot = max(chip_bottom, board_bottom) if side else chip_bottom
        chip_r = max(cx0 + chip_w, w - pad)
        if cr:
            cr.set_source_rgb(*chipc)
            cr.rectangle(cx0 - pad / 2, chip_top, chip_r - cx0 + pad / 2, chip_bot - chip_top)
            cr.fill()
            if not side:                            # the board band's chip edge
                cr.rectangle(x_pin - pad / 2, board_top - pad, w - pad - x_pin + pad / 2, board_bottom - board_top + pad)
                cr.fill()
        if ink is not None:
            ink.append(('chip edge', cx0 - pad / 2 - 1, chip_top, cx0 - pad / 2, chip_bot))
            if not side:
                ink.append(('board chip edge', x_pin - pad / 2 - 1, board_top - pad, x_pin - pad / 2, board_bottom))
        label(chip_r - tw('HY3131') - pad, chip_bot - L - pad * 0.3, 'HY3131', dim)

        # ---------- board ----------
        x = bx0
        rows = [('v', 'V Ω', C_RED), ('c', 'COM', C_BLACK), ('a', 'A', C_RED)]
        jy = {'v': jack_y['v'] if va else jack_y['n'], 'c': jack_y['c'], 'a': jack_y['a']}
        if not va:
            jy['v'] = board_top + L / 2
        x_relay = x + 2 * r + pad + name_w + pad * 2
        x_net = x_relay + relay_w + pad * 3
        for g, name, col in rows:
            yy = jy[g]
            if cr:
                cr.new_path()
                cr.set_source_rgb(*col)
                cr.arc(x + r, yy, r, 0, 6.3)
                cr.fill()
                cr.set_source_rgb(*txt)
                cr.arc(x + r, yy, r * 0.45, 0, 6.3)
                cr.fill()
            label(x + 2 * r + pad * 0.6, yy - L / 2, name, txt)

        def fan(x_from, y_from, pins, active, net):
            """a box output to each of its pins: one vertical track, a dot where it branches"""
            xm = x_pin - pad * 1.5
            ys_ = [py + L / 2 for p, py in pins if p != '–']
            if not ys_:
                return
            wire([(x_from, y_from), (xm, y_from)], active, net=net)
            lo, hi = min(ys_ + [y_from]), max(ys_ + [y_from])
            wire([(xm, lo), (xm, hi)], active, net=net)
            for p, py in pins:                       # each branch as its pin: on, time-shared (dashed), reserved
                if p != '–':
                    ps = pin_st.get(p, 'on')
                    wire([(xm, py + L / 2), (x_pin - pad * 0.3, py + L / 2)], active and ps != 'off', net=net,
                         dash=ps == 'shared')
            for yq in set(ys_ + [y_from]):
                deg = 1 + (yq > lo + 0.01) + (yq < hi - 0.01) + (1 if yq in ys_ and abs(yq - y_from) < 0.01 else 0)
                if deg >= 3:
                    dot(xm, yq, active)
        # V row
        wire([(x + 2 * r + pad * 0.6 + name_w + pad * 0.5, jy['v']), (x_relay, jy['v'])], True)
        bx = box(x_relay, jy['v'], relay_lines, True)
        # a jack net is live when a block reads one of its pins (the PA switch labels alone do not make it);
        # a pin only reserved for another state of the function (pin_st 'off') does not either
        v_on = any(not p.startswith('PA') and pin_st[p] != 'off' for p in pv)
        n_on = any(not p.startswith('PA') and pin_st[p] != 'off' for p in pn)
        if va:
            nb = box(x_net, jy['v'], net_v, v_on, minw=net_w - pad * 2)
            wire([(bx[2], jy['v']), (nb[0], jy['v'])], v_on)
            fan(nb[2], jy['v'], pys['v'], v_on, 'jv')
        else:
            yn = jack_y['n']
            wire([(bx[2], jy['v']), (bx[2] + pad * 1.5, jy['v']), (bx[2] + pad * 1.5, yn), (x_net, yn)], n_on)
            nb = box(x_net, yn, net_n, n_on, minw=net_w - pad * 2)
            fan(nb[2], yn, pys['n'], n_on, 'jv')
        # COM
        wire([(x + 2 * r + pad * 0.6 + name_w + pad * 0.5, jy['c']), (x_pin - pad * 1.5, jy['c'])], True, net='jc')
        fan(x_pin - pad * 1.5, jy['c'], pys['c'], True, 'jc')
        # A
        a_on = any(pin_st[p] != 'off' for p in pa_)
        wire([(x + 2 * r + pad * 0.6 + name_w + pad * 0.5, jy['a']), (x_relay, jy['a'])], True)
        sb = box(x_relay, jy['a'], sh_lines, a_on, minw=sh_w - pad * 2)
        fan(sb[2], jy['a'], pys['a'], a_on, 'ja')
        # pin names (inside the chip edge)
        pin_y = {}
        for g in ('v', 'n', 'c', 'a'):
            for p, py in pys[g]:
                act_ = p != '–' and pin_st.get(p) != 'off' and (
                    (g == 'v' and va and v_on) or (g == 'n' and not va and n_on) or g == 'c' or (g == 'a' and a_on))
                label(x_pin, py, p, txt if act_ else dim)
                if p != '–':
                    pin_y[p] = py + L / 2
        if not side:
            # the chip's own pin column: the pins its consumers select, one per line
            used = []
            for k_ in ('AD1', 'OP1', 'AD2', 'OP2', 'AD3', 'CMP'):
                for s_ in srcs[k_]:
                    if s_ in pin_y and s_ not in used:
                        used.append(s_)
            pin_y = {}
            yy = y_op1 - L
            for p in used:
                label(cx0, yy, p, txt if pin_st.get(p) != 'off' else dim)
                pin_y[p] = yy + L / 2
                yy += L + pad * 0.8
            x_pin = cx0

        # ---------- chip: consumers and lanes ----------
        xc = cx0 + pins_w + laneA                   # OP column
        x_conv = xc + op_w + laneB                  # converters
        cons_y = {'AD1': y_ad1, 'OP1': y_op1, 'AD2': y_ad2, 'OP2': y_op2, 'AD3': y_ad3, 'CMP': y_cmp}
        # opamps
        o1 = opamp(xc, y_op1, op1, 'OP1', ('×10' if flag('ops1') else '×1') if op1 else '', hold=True)
        o2 = opamp(xc, y_op2, op2, 'OP2', ('×10' if flag('ops2') else '×1') if op2 else '', hold=True)
        op_out = {'OP1O': (o1[1], y_op1, op1), 'OP2O': (o2[1], y_op2, op2)}
        # converters
        c1 = box(x_conv, y_ad1, b_ad1, ad1, minw=conv_w - pad * 2)
        c2 = box(x_conv, y_ad2, b_ad2, ad2, minw=conv_w - pad * 2)
        c3 = box(x_conv, y_ad3, b_ad3, ad3, minw=conv_w - pad * 2)
        cmpb = box(x_conv, y_cmp, b_cmp, cmp_used, minw=cmp_w - pad * 2)
        cbox = {'AD1': c1, 'AD2': c2, 'AD3': c3, 'CMP': cmpb}
        # Channel routing: every net (a pin, or an op-amp output) gets its own vertical track; the
        # source runs horizontally into the track, each consumer leaves it horizontally at its input's
        # height; tracks ordered for the fewest crossings; a dot where a track branches.
        nets_a, nets_b, unrouted = {}, {}, []
        for name in ('OP1', 'OP2', 'AD1', 'AD2', 'AD3', 'CMP'):
            x_in = (o1 if name == 'OP1' else o2)[0] if name.startswith('OP') else cbox[name][0]
            # every input the function's states read, each at its own height (stubs on top, then op-amp
            # outputs, then pins in board order: no crossings); the drawn set never changes inside a function
            ins = sorted(srcs[name], key=lambda s_: (s_ in pin_y, s_ in op_out, pin_y.get(s_, 0), s_))
            nin = len(ins)
            for i_, s_ in enumerate(ins):
                stt = src_st.get((name, s_), 'off')
                a_, dsh = stt != 'off', stt == 'shared'
                y_in = cons_y[name] + (i_ - (nin - 1) / 2) * L * 0.6
                if s_ in op_out and not name.startswith('OP'):
                    xo, yo, oa = op_out[s_]
                    nets_b.setdefault(s_, {'src': (xo, yo), 'dst': [], 'on': oa})['dst'].append(
                        (x_in, y_in, a_ and oa, dsh, nin == 1))
                elif s_ in pin_y:
                    nets_a.setdefault(s_, {'src': (x_pin + tw(s_) + pad * 0.4, pin_y[s_]), 'dst': [], 'on': True})[
                        'dst'].append((x_in, y_in, a_, dsh, nin == 1))
                elif s_:
                    # an internal node or a pin no jack net uses (PB6 ref, VDDA, TS): a stub label
                    sw = tw(s_)
                    lx0 = x_in - pad * 1.2 - sw
                    if lx0 > (cx0 + pins_w if name.startswith('OP') else xc + op_w):
                        label(lx0 - pad * 0.3, y_in - L - pad * 0.2, s_, dim if a_ else off)
                        wire([(lx0, y_in), (x_in, y_in)], a_, dash=dsh)
                        arrow(x_in, y_in, a_)
                    else:
                        unrouted.append(s_)

        def route(nets, x_first):
            names = list(nets)
            best = None
            for perm in itertools.permutations(range(len(names))):
                xs = {names[i]: x_first + step * perm[k] for k, i in enumerate(range(len(names)))}
                cost = 0
                for n1 in names:
                    for n2 in names:
                        if n1 == n2:
                            continue
                        x2 = xs[n2]
                        ys2 = [nets[n2]['src'][1]] + [d[1] for d in nets[n2]['dst']]
                        lo2, hi2 = min(ys2), max(ys2)
                        # n1's horizontals crossing n2's track
                        hs = [(nets[n1]['src'][0], xs[n1], nets[n1]['src'][1])] + \
                             [(xs[n1], d[0], d[1]) for d in nets[n1]['dst']]
                        for xa, xb, yh in hs:
                            if min(xa, xb) < x2 < max(xa, xb) and lo2 - 0.5 <= yh <= hi2 + 0.5:
                                cost += 1
                if best is None or cost < best[0]:
                    best = (cost, xs)
            return best[1] if best else {}

        def draw_nets(nets, xs):
            for n, d_ in nets.items():
                xt = xs[n]
                xs0, ys0 = d_['src']
                # a consumer within a few px of the source's height takes the wire straight (an op-amp or
                # a box accepts it anywhere along its input edge): no jog; not a block with several inputs
                d_['dst'] = [(x_in, ys0 if one and 0 < abs(y_in - ys0) < L * 0.7 else y_in, a_, dsh, one)
                             for x_in, y_in, a_, dsh, one in d_['dst']]
                any_on = any(dd[2] for dd in d_['dst'])
                wire([(xs0, ys0), (xt, ys0)], any_on, net=n)
                ys_ = [dd[1] for dd in d_['dst']]
                lo, hi = min(ys_ + [ys0]), max(ys_ + [ys0])
                # the track: blue between the source and the farthest active consumer
                act_ys = [dd[1] for dd in d_['dst'] if dd[2]]
                wire([(xt, lo), (xt, hi)], False, net=n)
                if act_ys:                          # the live part over it (same geometry, not re-recorded)
                    alo, ahi = min(act_ys + [ys0]), max(act_ys + [ys0])
                    wire([(xt, alo), (xt, ahi)], True, net=n, record=False)
                for x_in, y_in, a_, dsh, _one in d_['dst']:
                    wire([(xt, y_in), (x_in, y_in)], a_, net=n, dash=dsh)
                    arrow(x_in, y_in, a_)
                for yq in set(ys_ + [ys0]):
                    deg = 1 + (yq > lo + 0.01) + (yq < hi - 0.01) + sum(1 for v_ in ys_ + [ys0] if abs(v_ - yq) < 0.01) - 1
                    if deg >= 3:
                        dot(xt, yq, any_on)
        draw_nets(nets_a, route(nets_a, cx0 + pins_w + pad + step))
        draw_nets(nets_b, route(nets_b, xc + op_w + step))

        # ---------- processing ----------
        xp = x_conv + conv_w + pad * 4               # after the converters
        # AD1 -> result
        wire([(c1[2], y_ad1), (xp + hpf_w + pad * 3 + mul_r * 2 + pad * 3 + rms_w + pad * 2 - pad, y_ad1)], ad1)
        x_res = xp + hpf_w + pad * 3 + mul_r * 2 + pad * 3 + rms_w + pad * 2
        yy = y_ad1 - len(res1) * (L + pad / 2) / 2
        for s_ in res1:
            label(x_res, yy, s_, hot if ad1 else off)
            yy += L + pad / 2
        if stream:
            # AD2 results go straight to the MCU (R03..R05 stream, DMA)
            wire([(c2[2], y_ad2), (x_res - pad, y_ad2)], True, net='X')
            arrow(x_res - pad, y_ad2, True)
            label(x_res, y_ad2 - L / 2, 'stream → MCU  %s S/s' % si(num('ad2rate'), '').strip(), hot)
        # HPF on X (AD2) and Y (AD3)
        rms_on = flag('rms')
        h2 = box(xp, y_ad2, ['HPF'], ad2 and rms_on) if not stream else (xp, 0, xp + box_w(['HPF']), 0)
        h3 = box(xp, y_ad3, ['HPF'], ad3 and rms_on)
        if not stream:
            wire([(c2[2], y_ad2), (h2[0], y_ad2)], ad2 and rms_on, net='X')
        wire([(c3[2], y_ad3), (h3[0], y_ad3)], ad3 and rms_on, net='Y')
        label(h2[2] + pad * 0.8, y_ad2 - L - pad * 0.5, 'X', txt if ad2 else off)
        label(h3[2] + pad * 0.8, y_ad3 - L - pad * 0.5, 'Y', txt if ad3 else off)
        # multiplier
        mx = h2[2] + pad * 3 + mul_r
        my = y_op2
        x_used = stat in (0, 1)
        y_used = stat in (1, 2)
        if not stream:
            wire([(h2[2], y_ad2), (mx, y_ad2), (mx, my - mul_r)], rms_on and x_used)
        wire([(h3[2], y_ad3), (mx, y_ad3), (mx, my + mul_r)], rms_on and y_used)
        if cr:
            cr.new_path()
            cr.set_source_rgb(*bg)
            cr.arc(mx, my, mul_r, 0, 6.3)
            cr.fill_preserve()
            cr.set_source_rgb(*(on if rms_on else off))
            cr.set_line_width(lw)
            cr.stroke()
            d = mul_r * 0.5
            cr.move_to(mx - d, my - d)
            cr.line_to(mx + d, my + d)
            cr.move_to(mx - d, my + d)
            cr.line_to(mx + d, my - d)
            cr.stroke()
        if ink is not None:
            ink.append(('mul', mx - mul_r - 1, my - mul_r - 1, mx + mul_r + 1, my + mul_r + 1))
        mst_w = max(tw(m_) for m_ in ('X·X', 'X·Y', 'Y·Y'))     # the power meter cycles it: a fixed place
        label(mx - mul_r - pad * 0.8 - mst_w, my - L / 2, mstat, hot if rms_on else off)
        rb = box(mx + mul_r + pad * 3, my, rms_lines, rms_on, minw=max(tw('Σ %s / N' % m_) for m_ in ('X·X', 'X·Y', 'Y·Y')))
        wire([(mx + mul_r, my), (rb[0], my)], rms_on)
        arrow(rb[0], my, rms_on)
        if rms_res and rms_on:
            label(x_res, my - L - pad / 4, rms_res, hot)
            label(x_res, my + pad / 4, 'RMS %s' % rmscode, dim)
        elif rms_on:
            label(x_res, my - L / 2, 'RMS %s' % rmscode, hot)
        wire([(rb[2], my), (x_res - pad, my)], rms_on)
        # LPF (mean of X or Y)
        lpf_on = flag('lpf')
        tap2, tap3 = c2[2] + pad * 1.5, c3[2] + pad * 2.8
        lb = box(xp, y_lpf, lpf_lines, lpf_on, minw=max(tw('X, OSR 16384'), tw('Y, OSR 16384')))     # OSR text: the box keeps its size
        wire([(tap2, y_ad2), (tap2, y_lpf - L * 0.3), (lb[0], y_lpf - L * 0.3)], lpf_on and not lpf_y, net='X')
        wire([(tap3, y_ad3), (tap3, y_lpf + L * 0.3), (lb[0], y_lpf + L * 0.3)], lpf_on and lpf_y, net='Y')
        dot(tap2, y_ad2, lpf_on and not lpf_y)       # the LPF taps branch off the converter outputs
        dot(tap3, y_ad3, lpf_on and lpf_y)
        wire([(lb[2], y_lpf), (x_res - pad, y_lpf)], lpf_on)
        # peak hold: ENPKH drops for a few ms at each restart, the probe brings one: the slot is held
        pk_slot = any(v.get('pkh') == '1' for v in ctx['variants'])
        pk = flag('pkh') or ctx['pkh_recent']
        label(x_res, y_lpf - (L + pad / 4 if pk_slot else L / 2), lpf_res if lpf_on else 'LPF off', hot if lpf_on else off)
        # peak hold (its source by PKHSEL)
        if pk_slot and not pk:
            label(x_res, y_lpf + pad / 4, 'peak hold off', off)
        elif pk:
            label(x_res, y_lpf + pad / 4, 'peak hold %s: %s … %s' % (
                ['AD2', 'AD1', 'LPF', 'LPF'][int(num('pkhsel'))], st.get('pkmin', ''), st.get('pkmax', '')), dim)
        # comparator -> counter
        ctr_on = flag('ctr')
        kb = box(cmpb[2] + pad * 3, y_cmp, ['counter'], ctr_on)
        wire([(cmpb[2], y_cmp), (kb[0], y_cmp)], ctr_on)
        wire([(kb[2], y_cmp), (x_res - pad, y_cmp)], ctr_on)
        label(x_res, y_cmp - L / 2, ('f %s' % si(num('ctf'), 'Hz')) if ctr_on else
              ('CMP pin → beeper' if flag('cmppin') else 'counter off'), hot if ctr_on or flag('cmppin') else off)
        if 'cmp' in alt:                            # under the CMP box: what the two inputs are for
            label(x_conv, cmpb[3] + max(4.5, pad * 0.6), self.alt_note(st, 'cmp', alt['cmp']), dim)
        # power: what the MCU makes of it
        y_end = max(chip_bottom, board_bottom)
        y_end = self.source_band(st, y_end, w, pad, L, lw, tw, label, wire, arrow, box, opamp, dot, si, num, ink,
                                 cr, on, off, hot, txt, dim, bg)
        if power:
            if 'rd_ad1idc' in st and 'rd_vdc' in st:
                note = 'DC power: P = V dc (AD3 → LPF, Y) × I dc (AD1) = %s' % si(num('rd_vdc') * num('rd_ad1idc'), 'W')
            else:
                note = 'AC power: P = mean(X·Y) of AD2 (I, OP1 on the shunt) and AD3 (V, OP2 on the tap); ' \
                       'V rms / I rms from Y·Y / X·X, the MCU cycles the statistic (SVXI / SDO23)'
            y_end += pad
            y_end += label(pad, y_end, note, hot, w - 2 * pad)
        if unrouted:
            y_end += label(pad, y_end, 'other inputs: ' + ', '.join(unrouted), dim, w - 2 * pad)
        # AD1's input alternation: its line is held while the function can switch it (empty until seen)
        pred = self.alt_sort(st, 'ad1', [self.gval(v, 'ad1') for v in ctx['variants']])
        if 'ad1' in alt or len(pred) > 1:
            y_end += pad
            hold = label(pad, y_end, self.alt_note(st, 'ad1', pred), dim, w - 2 * pad, draw=False)
            if 'ad1' in alt:
                hold = max(hold, label(pad, y_end, self.alt_note(st, 'ad1', alt['ad1']), dim, w - 2 * pad))
            y_end += hold
        if ctx['other'] is not None:
            y_end += pad
            y_end += label(pad, y_end, 'AC+DC: the firmware alternates the %s and %s rows (~1 s each); the drawing '
                           'holds both, the other row\'s inputs grey' % (st.get('role'), ctx['other'].get('role')),
                           dim, w - 2 * pad)
        if self.pa_switches(st):
            y_end += pad
            y_end += label(pad, y_end, 'PA<n> switches (R2A..R2E): P power bus (SMODE %s), D OP3 output, F FB, S SENSE'
                           % self.SMODE.get(int(st.get('smode', '0'), 16) & 15, '?'), dim, w - 2 * pad)
        return y_end + pad


class FrontendWindow(Adw.Window):
    def __init__(self, parent, st):
        super().__init__(title='DM40 front end', transient_for=parent)
        self.set_default_size(900, 820)
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        box.append(Adw.HeaderBar())
        self.view = FrontendView(fit=True)
        self.view.set_vexpand(True)
        self.view.update(st)
        scroll = Gtk.ScrolledWindow(vexpand=True, hscrollbar_policy=Gtk.PolicyType.NEVER)
        scroll.set_child(self.view)
        box.append(scroll)
        self.set_content(box)

    def update(self, st):
        self.view.update(st)


class Field:
    """A compact 'label  widget' row of an input card; delegates to the widget (get_text, get_selected,
    get_active ...), set_title / set_visible act on the whole row."""

    def __init__(self, title, widget, sizegroup):
        self.widget = widget
        self.label = Gtk.Label(label=title, xalign=0)
        sizegroup.add_widget(self.label)
        if not isinstance(widget, Gtk.Switch):
            widget.set_hexpand(True)
        self.row = Gtk.Box(spacing=8)
        self.row.append(self.label)
        if isinstance(widget, Gtk.Switch):
            self.row.append(Gtk.Box(hexpand=True))
        self.row.append(widget)

    def set_title(self, t):
        self.label.set_label(t)

    def set_visible(self, v):
        self.row.set_visible(v)

    def __getattr__(self, name):
        return getattr(self.widget, name)


class GtkApp(Adw.Application):
    def __init__(self, args, extra, emulator_cls):
        super().__init__(application_id='win.xcvb.dm40emu')
        self.args, self.extra, self.emulator_cls = args, extra, emulator_cls
        self.emu = None
        self.starting = False
        self.key_down = {}
        self.rate_hist = []
        self.last_image = None
        self.closing = False
        self.connect('activate', self.on_activate)
        self.connect('shutdown', lambda *a: self.stop_emulator())

    def open_frontend_window(self):
        if self.fe_big is None:
            self.fe_big = FrontendWindow(self.win, self.fe_view.st)
            self.fe_big.connect('close-request', lambda *a: setattr(self, 'fe_big', None) or False)
        self.fe_big.present()

    # ---- window ----
    def on_activate(self, app):
        self.win = Adw.ApplicationWindow(application=self, title='DM40 emulator')
        self.win.set_default_size(WIN_W, WIN_H)
        self.title = Adw.WindowTitle(title='DM40 emulator', subtitle='starting…')
        header = Adw.HeaderBar()
        header.set_title_widget(self.title)

        self.fw = Gtk.DropDown.new_from_strings(['stock', 'fwrs', 'chain'])
        self.fw.set_selected(['stock', 'fwrs', 'chain'].index(self.args.mode))
        self.fw.set_tooltip_text('firmware to run')
        self.fw.connect('notify::selected', lambda *a: self.restart())
        header.pack_start(self.fw)
        # DM40 model (run.py --model: calibration model byte, both firmwares;
        # default = the dump's own, or --model given to live.py).
        m = self.extra.index('--model') if '--model' in self.extra else -1
        b = self.extra.index('--base') + 1 if '--base' in self.extra else -1
        base = self.extra[b] if 0 < b < len(self.extra) else run.DUMP
        model = self.extra[m + 1].upper() if 0 <= m < len(self.extra) - 1 else run.flash_model(run.read(base))
        self.extra = self.extra[:m] + self.extra[m + 2:] if m >= 0 else self.extra
        self.model_dd = Gtk.DropDown.new_from_strings(['DM40A', 'DM40B', 'DM40C'])
        self.model_dd.set_selected('ABC'.index(model))
        self.model_dd.set_tooltip_text('model byte of the calibration record (40000 / 50000 / 60000 counts in the stock)')
        self.model_dd.connect('notify::selected', lambda *a: self.restart())
        header.pack_start(self.model_dd)
        self.fast = Gtk.ToggleButton(label='Fast')
        self.fast.set_tooltip_text('PerformanceInMips 50: about 2x faster, UI work takes longer in virtual time')
        self.fast.set_active(self.args.fast)
        self.fast.connect('toggled', lambda *a: self.restart())
        header.pack_start(self.fast)
        self.cap = Gtk.ToggleButton(label='≤1x', active=not getattr(self.args, 'no_realtime_cap', False))
        self.cap.set_tooltip_text('real-time cap: virtual time never runs ahead of the wall clock '
                                  '(off: as fast as the host allows; live.py --no-realtime-cap)')
        header.pack_start(self.cap)
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

        self.panel = FrontPanel(self.on_panel_event)
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        content.append(header)
        # The device picture at an integer scale top-left; the input cards fill the rest: DMM jacks and
        # the front-end schematic in the right column, scope / generator+USB / monitor under the device
        # when there is room there (else at the end of the right column, which scrolls).
        # a Paned: the handle between the device and the card column is draggable; the device scales
        # to the left side (on_body_tick)
        body = Gtk.Paned(orientation=Gtk.Orientation.HORIZONTAL, wide_handle=True, vexpand=True)
        self.body = body
        self.side_set = False
        left = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, valign=Gtk.Align.START)
        left.set_hexpand(False)               # not inherited from the cards under the device
        left.append(self.panel)
        self.flow_slot = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, margin_start=PANEL_MARGIN,
                                 margin_end=PANEL_MARGIN, margin_bottom=CARD_GAP)
        left.append(self.flow_slot)
        self.bottom = Gtk.Box(spacing=CARD_GAP, margin_start=PANEL_MARGIN,
                              margin_end=PANEL_MARGIN, margin_bottom=CARD_GAP)
        # a non-scrolling ScrolledWindow: natural width = minimum width, so the cards never widen the
        # column beyond the device picture
        self.bottom_wrap = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER,
                                              vscrollbar_policy=Gtk.PolicyType.NEVER, propagate_natural_height=True)
        self.bottom_wrap.set_child(self.bottom)
        left.append(self.bottom_wrap)
        # the left side scrolls: the device, then the signal-flow diagram under it
        left_scroll = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER, vexpand=True)
        left_scroll.set_child(left)
        body.set_start_child(left_scroll)
        body.set_resize_start_child(True)
        body.set_shrink_start_child(False)
        self.right = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=CARD_GAP, margin_start=CARD_GAP // 2,
                             margin_end=CARD_GAP, margin_top=CARD_GAP, margin_bottom=CARD_GAP)
        scroll = Gtk.ScrolledWindow(hexpand=True, vexpand=True, hscrollbar_policy=Gtk.PolicyType.NEVER)
        scroll.set_size_request(SIDE_MIN_W, -1)
        scroll.set_child(self.right)
        body.set_end_child(scroll)
        body.set_resize_end_child(True)
        body.set_shrink_end_child(False)
        self.cards = self.build_cards()
        self.right.append(self.cards['dmm'])
        self.flow_slot.append(self.cards['fe'])     # under the device (the wide free area)
        self.bottom_cards = [self.cards['scope'], self.cards['gen'], self.cards['mon']]
        for c in self.bottom_cards:
            c.set_hexpand(True)
            self.right.append(c)
        self.cards_below = False
        self.bottom_wrap.set_visible(False)
        self.body_size = None
        body.add_tick_callback(self.on_body_tick)
        content.append(body)
        self.win.set_content(content)

        keys = Gtk.EventControllerKey()
        keys.connect('key-pressed', self.on_key_pressed)
        keys.connect('key-released', self.on_key_released)
        self.win.add_controller(keys)
        self.win.connect('close-request', self.on_close)
        for sig in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
            GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, sig, self.on_close)
        self.win.present()
        self.start_emulator()
        GLib.timeout_add(33, self.refresh)

    # ---- emulator ----
    def extra_args(self):
        extra = list(self.extra) + ['--model', 'ABC'[self.model_dd.get_selected()]]
        self.panel.model = 'ABC'[self.model_dd.get_selected()]
        self.panel.queue_draw()
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
        if self.usb_sub:
            self.usb_note.set_label('USB: ' + self.usb_sub + (
                '. Stock: only after Settings > Other > USB Connect (and even then it does not enumerate in the '
                'emulator): until then "no answer" after 5 s' if mode == 'stock' else ''))
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
        if self.closing and not isinstance(emu, Exception):
            emu.stop()                    # the window closed while Renode was starting
            return False
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
        self.closing = True
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
        self.emu.cap = self.cap.get_active()
        self.emu.pace(vus)
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
            self.reply.set_label('last reply: ' + re.sub(r'\[[0-9;]*m', '', self.emu.last_reply))
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
            # focus the card's first control: the scrolled column brings it into view
            target = {'v': self.focus_dmm, 'com': self.focus_dmm, 'a': self.focus_dmm,
                      'scope': self.focus_scope, 'gen': self.focus_gen, 'usb': self.focus_gen,
                      'led': self.focus_gen}[data]
            target.grab_focus()

    # ---- input cards (DMM jacks, front end, scope, generator / USB, monitor) ----
    @staticmethod
    def card(title, suffix=None):
        """A titled card: -> (outer widget, content box)."""
        outer = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        outer.add_css_class('card')
        box = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=6, margin_start=12, margin_end=12,
                      margin_top=8, margin_bottom=10)
        head = Gtk.Box(spacing=6)
        lbl = Gtk.Label(label=title, xalign=0, hexpand=True)
        lbl.add_css_class('heading')
        head.append(lbl)
        if suffix:
            head.append(suffix)
        box.append(head)
        outer.append(box)
        return outer, box

    @staticmethod
    def note(text='', lines=0):
        lbl = Gtk.Label(label=text, xalign=0, wrap=True, selectable=True, max_width_chars=34)
        lbl.set_wrap_mode(Pango.WrapMode.WORD_CHAR)
        lbl.add_css_class('dim-label')
        lbl.add_css_class('caption')
        if lines:
            lbl.set_lines(lines)
            lbl.set_ellipsize(Pango.EllipsizeMode.END)
        return lbl

    def field(self, box, sg, title, widget):
        f = Field(title, widget, sg)
        box.append(f.row)
        return f

    def entry(self, box, sg, title, text):
        e = Gtk.Entry(text=text, width_chars=7, max_width_chars=12)
        e.connect('activate', lambda *a: self.apply_dmm())
        return self.field(box, sg, title, e)

    @staticmethod
    def make_dropdown(items):
        """A DropDown whose button ellipsizes the selected item (narrow columns); the popup shows it whole."""
        d = Gtk.DropDown.new_from_strings(items)

        def setup(_f, item, ellipsize):
            item.set_child(Gtk.Label(xalign=0, ellipsize=Pango.EllipsizeMode.END if ellipsize else Pango.EllipsizeMode.NONE))

        def bind(_f, item):
            item.get_child().set_label(item.get_item().get_string())
        for setter, ell in ((d.set_factory, True), (d.set_list_factory, False)):
            f = Gtk.SignalListItemFactory()
            f.connect('setup', setup, ell)
            f.connect('bind', bind)
            setter(f)
        return d

    def dropdown(self, box, sg, title, items, on_change):
        d = self.make_dropdown(items)
        d.connect('notify::selected', lambda *a: on_change())
        return self.field(box, sg, title, d)

    def switch(self, box, sg, title, on_change, active=False):
        s = Gtk.Switch(active=active, halign=Gtk.Align.END, valign=Gtk.Align.CENTER, hexpand=False)
        s.connect('notify::active', lambda *a: on_change())
        return self.field(box, sg, title, s)

    def build_cards(self):
        """-> {name: card}; the window lays them out next to / under the device."""
        cards = {}
        # DMM jacks: what is connected to V-ohm / COM / A (DM40 front-end model, emu/hy3131/README.md).
        # The firmware's relays and HY3131 switches route it; the schematic shows the route.
        outer, box = self.card('DMM jacks')
        cols = Gtk.FlowBox(selection_mode=Gtk.SelectionMode.NONE, max_children_per_line=2, min_children_per_line=1,
                           homogeneous=True, column_spacing=18, row_spacing=6)
        vcol = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        acol = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=4)
        cols.append(vcol)
        cols.append(acol)
        for ch in (cols.get_child_at_index(0), cols.get_child_at_index(1)):
            ch.set_focusable(False)
            ch.set_valign(Gtk.Align.START)
        box.append(cols)
        sv = Gtk.SizeGroup(mode=Gtk.SizeGroupMode.HORIZONTAL)
        self.v_kinds = ['nothing', 'DC voltage source', 'AC voltage source', 'resistor', 'capacitor', 'diode',
                        'thermocouple (type K)', 'short', 'current source (wrong jack)']
        self.v_kind = self.dropdown(vcol, sv, 'V Ω / COM', self.v_kinds, self.dmm_fields_changed)
        self.v_kind.set_selected(1)
        self.v_val = self.entry(vcol, sv, 'Value', '1.234')
        self.v_ac = self.entry(vcol, sv, 'AC, V rms', '0')
        self.v_f = self.entry(vcol, sv, 'Frequency, Hz', '50')
        self.v_shape = self.dropdown(vcol, sv, 'Shape', ['sine', 'square', 'triangle'], self.dmm_fields_changed)
        self.v_duty = self.entry(vcol, sv, 'Duty 0..1', '0.5')
        self.v_rs = self.entry(vcol, sv, 'Source R, Ω', '0')
        self.v_rev = self.switch(vcol, sv, 'Cathode at V', self.apply_dmm)
        sa = sv                       # one label column width for both jack columns
        self.a_kinds = ['nothing', 'DC current source', 'AC current source', 'voltage source (wrong jack)']
        self.a_kind = self.dropdown(acol, sa, 'A / COM', self.a_kinds, self.dmm_fields_changed)
        self.a_val = self.entry(acol, sa, 'Value', '0.0123')
        self.a_ac = self.entry(acol, sa, 'AC, A rms', '0')
        self.a_f = self.entry(acol, sa, 'Frequency, Hz', '50')
        self.a_ph = self.entry(acol, sa, 'Phase lag, °', '0')
        self.a_compl = self.entry(acol, sa, 'Compliance, V', '10')
        self.cj = self.entry(acol, sa, 'Cold junction, °C', '25')
        self.die_follows = self.switch(acol, sa, 'Die = cold junction', self.dmm_fields_changed, active=True)
        self.die = self.entry(acol, sa, 'HY3131 die, °C', '25')
        row = Gtk.Box(spacing=10)
        b = Gtk.Button(label='Apply', valign=Gtk.Align.START)
        b.set_tooltip_text('send the connection (Enter in a field does the same)')
        b.connect('clicked', lambda *a: self.apply_dmm())
        row.append(b)
        self.dmm_spec = self.note()
        self.dmm_spec.set_hexpand(True)
        row.append(self.dmm_spec)
        box.append(row)
        cards['dmm'] = outer
        self.focus_dmm = self.v_kind.widget
        # Front end: the route the firmware selected (relays, HY3131 switches), live from the state file
        big = Gtk.Button(icon_name='view-fullscreen-symbolic', valign=Gtk.Align.CENTER)
        big.set_tooltip_text('open the schematic in its own resizable window')
        big.add_css_class('flat')
        big.connect('clicked', lambda *a: self.open_frontend_window())
        outer, box = self.card('Signal flow: jacks → HY3131 → results', big)
        self.fe_view = FrontendView()
        self.fe_view.set_tooltip_text('the route the firmware chose (relays, HY3131 muxes and switches)')
        box.append(self.fe_view)
        self.fe_big = None
        self.fe_row = self.note('-', lines=3)    # one-line summary (the schematic shows it all): not shown
        self.fe_row.set_visible(False)
        box.append(self.fe_row)
        cards['fe'] = outer
        self.fe_state_path = os.path.join(run.OUT, 'live-frontend-%d.txt' % os.getpid())
        self.fe_state_sent = None
        self.fe_last = ''
        # Scope input
        b = Gtk.Button(label='Apply', valign=Gtk.Align.CENTER)
        b.connect('clicked', lambda *a: self.apply_wave())
        outer, box = self.card('Scope input', b)
        grid = Gtk.Grid(column_spacing=6, row_spacing=4)
        self.wave = self.make_dropdown(['sine', 'square', 'triangle', 'ramp', 'dc', 'noise'])
        self.freq, self.vpp, self.voff = (Gtk.Entry(text=t, width_chars=6, max_width_chars=7, tooltip_text=tip)
                                          for t, tip in (('1000', 'frequency, Hz'), ('1.0', 'amplitude, V peak-to-peak'),
                                                         ('0', 'offset, V')))
        for e in (self.freq, self.vpp, self.voff):
            e.connect('activate', lambda *a: self.apply_wave())
        # [sine]        [1000.0] Hz
        # [1.000] Vpp   [0.000]  V offset
        for col, row, wdg in ((0, 0, self.wave), (2, 0, self.freq), (3, 0, 'Hz'),
                              (0, 1, self.vpp), (1, 1, 'Vpp'), (2, 1, self.voff), (3, 1, 'V offset')):
            if isinstance(wdg, str):
                wdg = Gtk.Label(label=wdg, xalign=0)
            else:
                wdg.set_hexpand(True)
            grid.attach(wdg, col, row, 2 if wdg is self.wave else 1, 1)
        box.append(grid)
        cards['scope'] = outer
        self.focus_scope = self.wave
        # Generator / USB
        outer, box = self.card('Generator, USB')
        gen = self.note('Generator: no analog output yet')
        gen.set_tooltip_text('the DAC / TIM6 / TIM12 registers run; the MCX OUT signal is not modelled')
        box.append(gen)
        port = getattr(self.args, 'usbip', None)
        if port:
            env = '' if port == 3240 else 'USBIP_PORT=%d ' % port
            sub = 'USB/IP 127.0.0.1:%d, busid 1-0 (hover: how to connect)' % port
            tip = ('%semu/usbip_client.py list | desc 1-0 | usbtmc 1-0 "*IDN?" | scpi-tcp 1-0 5025 | msc 1-0; '
                   'or sudo usbip attach -r 127.0.0.1 -b 1-0' % env)
        else:
            sub, tip = 'not exported (live.py --no-usbip)', None
        self.usb_sub = sub if port else None
        self.usb_note = self.note('USB: ' + sub)
        self.usb_note.set_tooltip_text(tip)
        box.append(self.usb_note)
        cards['gen'] = outer
        self.focus_gen = self.usb_note
        # Monitor
        outer, box = self.card('Renode monitor')
        self.mon = Gtk.Entry(placeholder_text='command, e.g. sysbus.hy3131 Frontend', hexpand=True)
        self.mon.connect('activate', lambda e: self.send(e.get_text()))
        box.append(self.mon)
        self.reply = self.note('last reply: -', lines=3)
        box.append(self.reply)
        cards['mon'] = outer
        self.cards_ready = True
        self.dmm_fields_changed()
        return cards

    def on_body_tick(self, body, _clock):
        """Window size changed: the device's scale, and where the small cards go."""
        W, H = body.get_width(), body.get_height()
        if W <= 0:
            return True
        sf = self.win.get_scale_factor()
        pw1 = DEV_W * PX_PER_MM_1X / sf + 2 * PANEL_MARGIN
        ph1 = PANEL_H * PX_PER_MM_1X / sf + 2 * PANEL_MARGIN
        if not self.side_set:
            # first layout: the card column ~30 % (at least SIDE_MIN_W; the device keeps scale 1 while
            # it can); afterwards the user drags the handle
            self.side_set = True
            body.set_position(int(W - max(SIDE_MIN_W, min(0.3 * W, W - pw1))))
            return True
        LW = body.get_position()
        if (W, H, LW) == self.body_size:
            return True
        self.body_size = (W, H, LW)
        k = max(0.5, min(LW / pw1, H / ph1))
        # shrink the device a little (<= 15 %) when that lets the small cards sit under it
        each = (DEV_W * PX_PER_MM_1X * k / sf - 2 * CARD_GAP) / len(self.bottom_cards)
        need = max(c.measure(Gtk.Orientation.VERTICAL, max(1, int(each)))[1] for c in self.bottom_cards)
        k2 = (H - need - CARD_GAP - 2 * PANEL_MARGIN) / (PANEL_H * PX_PER_MM_1X / sf)
        if False and max(0.85 * k, min(1.0, k)) <= k2 < k:   # (the cards no longer go under the device)
            k = k2
        if abs(k - self.panel.want_k) > 1e-3:
            self.panel.want_k = k
            self.panel.queue_resize()
        # the small cards go under the device when they fit there, else into the right column
        pw = DEV_W * PX_PER_MM_1X * k / sf
        each = (pw - 2 * CARD_GAP) / len(self.bottom_cards)
        need_h = max(c.measure(Gtk.Orientation.VERTICAL, int(each))[1] for c in self.bottom_cards)
        need_w = sum(c.measure(Gtk.Orientation.HORIZONTAL, -1)[0] for c in self.bottom_cards)
        below = H - PANEL_H * PX_PER_MM_1X * k / sf - 2 * PANEL_MARGIN - CARD_GAP
        # the signal-flow diagram takes the space under the device: the small cards stay on the right
        want = False and below >= need_h and pw >= need_w + 2 * CARD_GAP
        if want != self.cards_below:
            self.cards_below = want
            GLib.idle_add(self.reflow_cards)
        return True

    def reflow_cards(self):
        target = self.bottom if self.cards_below else self.right
        for c in self.bottom_cards:
            if c.get_parent() is not target:
                c.get_parent().remove(c)
                target.append(c)
        self.bottom_wrap.set_visible(self.cards_below)
        return False

    V_DEFAULTS = {'DC voltage source': '1.234', 'AC voltage source': '0', 'resistor': '4700', 'capacitor': '100e-9',
                  'diode': '0.65', 'thermocouple (type K)': '100', 'current source (wrong jack)': '0.001'}
    A_DEFAULTS = {'DC current source': '0.0123', 'AC current source': '0', 'voltage source (wrong jack)': '0.5'}

    def dmm_fields_changed(self):
        """Show only the fields of the chosen sources (with a typical value when the kind changes), then apply."""
        if not getattr(self, 'cards_ready', False):
            return
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
        self.a_ph.set_visible(ak == 'AC current source')
        # lag behind the V source's AC part (PF = cos); only meaningful with an AC V source
        self.a_ph.set_tooltip_text('current lags the V source by this angle: + inductive, − capacitive; PF = cos')
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
            parts.append('isrc:%s,ac=%s,f=%s,compl=%s,ph=%s' % (a, self.num(self.a_ac, '0'), self.num(self.a_f, '50'),
                                                               self.num(self.a_compl, '10'), self.num(self.a_ph, '0')))
        elif ak == 'voltage source (wrong jack)':
            parts.append('avsrc:%s' % a)
        parts.append('cj:%s' % self.num(self.cj, '25'))
        parts.append('die:none' if self.die_follows.get_active() else 'die:%s' % self.num(self.die, '25'))
        return ';'.join(parts)

    def apply_dmm(self):
        spec = self.dmm_spec_text()
        self.dmm_spec.set_label(spec)
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
            if self.fe_big:
                self.fe_big.update(st)
            self.fe_row.set_label('%s  shunt %s  PC7 %s · %s mode %s range %s · AD1 %s AD2 %s AD3 %s LPF %s RMS %s CTR %s · '
                                     'Vjack %s V  Ijack %s A%s' % (
                                         'V/A path' if st.get('pa') == '0' else 'Ω/mV network', st.get('shunt'), st.get('pc7'),
                                         st.get('role'), st.get('mode'), st.get('range'), st.get('ad1'), st.get('ad2'), st.get('ad3'),
                                         st.get('lpf'), st.get('rms'), st.get('ctr'), st.get('vjack'), st.get('ijack'),
                                         ('  node %s V' % st['vx']) if 'vx' in st else ''))

    def apply_wave(self):
        shape = self.wave.get_selected_item().get_string()
        self.send('sysbus.scopefeed SetWave "%s" %g %g %g' % (
            shape, float(self.num(self.freq, '1000')), float(self.num(self.vpp, '1')), float(self.num(self.voff, '0'))))

    def on_key_pressed(self, ctrl, keyval, keycode, state):
        name = Gdk.keyval_name(keyval) or ''
        low = name.lower()
        if low in ('escape',) or (low == 'c' and state & Gdk.ModifierType.CONTROL_MASK):
            self.on_close()
            return True
        if isinstance(self.win.get_focus(), (Gtk.Text, Gtk.Entry)):
            return False  # typing in a card
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
        self.emu.held = False             # the user's pause / start replaces the cap's
        btn.set_icon_name('media-playback-start-symbolic' if paused else 'media-playback-pause-symbolic')
        self.send('pause' if paused else 'start')


def run_app(args, extra, emulator_cls):
    app = GtkApp(args, extra, emulator_cls)
    try:
        return app.run([])
    finally:
        app.closing = True
        app.stop_emulator()
        # a start still in progress (building / launching): wait for it and stop what it started;
        # the Renode watchdog (live.py WATCHDOG) covers a hard exit
        t = time.time() + 120
        while app.starting and time.time() < t:
            time.sleep(0.1)
            GLib.MainContext.default().iteration(False)
        app.stop_emulator()
