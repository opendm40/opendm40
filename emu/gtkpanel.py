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
PANEL_Y0, PANEL_H = TOPV_Y, DEV_H - TOPV_Y   # drawn area: top view + gap + front view
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
        self.want_k = 1               # integer scale the window picked (GtkApp.on_body_tick)
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
        k = max(1, int(min(aw * sf / (DEV_W * PX_PER_MM_1X), ah * sf / (PANEL_H * PX_PER_MM_1X))))
        s = PX_PER_MM_1X * k / sf
        ox = (aw - DEV_W * s) / 2
        oy = (ah - PANEL_H * s) / 2 - PANEL_Y0 * s
        return k, s, ox, oy

    def do_measure(self, orientation, for_size):
        s = PX_PER_MM_1X * self.want_k / self.get_scale_factor()
        n = int((DEV_W if orientation == Gtk.Orientation.HORIZONTAL else PANEL_H) * s) + 1
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
            if (mx - cx) ** 2 + (my - cy) ** 2 <= (JACK_R + 0.8) ** 2:
                return 'jack', name
        for name, r in TOP.items():
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
        self.text(cr, GLASS[0] + GLASS[2] - 2.5, 12.85, 'DM40C', 2.6, C_STRIP_TEXT, align='right', bold=True)
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
        self.inside_h = 0
        self.set_content_height(120)
        self.set_draw_func(self.draw)

    def update(self, st):
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

    def layout(self, cr, w):
        """Draw (cr) or only measure (cr None) at width w; returns the height used. Board part: jacks ->
        relays -> divider / Ω-mV network / shunt -> the HY3131 pins they land on; chip part: every mux with
        its options (the selected one filled), the blocks it feeds (on / off), the PA switch matrix."""
        st = self.st
        if cr is not None:
            self.layout(None, w)               # measure first: the chip box is drawn behind its content
        on, off, hot, txt = rgb('#3584E4'), rgb('#6E6E6E'), rgb('#F3B51E'), rgb('#E8E8E8')
        dim, chipc = rgb('#9A9A9A'), rgb('#2A2C31')
        pl = self.create_pango_layout('Ag')
        L = pl.get_pixel_size()[1]          # line height of the widget font
        pad = L * 0.4
        ink = self.ink if cr is not None else None

        def tw(s):
            pl.set_text(s, -1)
            return pl.get_pixel_size()[0]

        def label(x, y, s, color=txt, width=None, mark=True):
            """text with its top-left at x, y; wraps at width; returns its height"""
            pl.set_text(s, -1)
            pl.set_width(int(width * Pango.SCALE) if width else -1)
            pl.set_wrap(Pango.WrapMode.WORD_CHAR)
            tw_, hgt = pl.get_pixel_size()
            if cr:
                cr.set_source_rgb(*color)
                cr.move_to(x, y)
                PangoCairo.show_layout(cr, pl)
            if ink is not None and mark and s.strip():
                ink.append((s, x, y, x + tw_, y + hgt))
            pl.set_width(-1)
            return hgt

        def line(x0, y0, x1, y1, active):
            if cr:
                cr.set_source_rgb(*(on if active else off))
                cr.move_to(x0, y0)
                cr.line_to(x1, y1)
                cr.stroke()

        def box(x, yc, bw, s, active):
            bh = L + pad * 1.5
            if cr:
                cr.set_source_rgb(*(on if active else off))
                cr.rectangle(x, yc - bh / 2, bw, bh)
                cr.stroke()
            label(x + (bw - tw(s)) / 2, yc - L / 2, s, txt if active else off, mark=False)
            if ink is not None:
                ink.append(('box ' + s, x - 1, yc - bh / 2 - 1, x + bw + 1, yc + bh / 2 + 1))

        if cr:
            cr.set_line_width(max(1.5, L / 9))
        if not st:
            return label(pad, pad, 'waiting for the HY3131 state …', txt, w - 2 * pad) + 2 * pad

        pa = st.get('pa', '0')
        role = st.get('role', 'None')
        shunt = st.get('shunt', 'uA')
        rng = st.get('range', '0')
        rv = role in ('DcV', 'AcV')
        rmv = role in ('DcMv', 'AcMv', 'Freq', 'Temp')
        rohm = role in ('Ohm', 'Cont', 'Diode', 'CapOsc', 'CapRamp', 'CapDischarge')
        ra = role in ('DcA', 'AcA')
        va = pa == '0'
        divs = {'0': '/10', '1': '/100', '2': '/1000', '3': '/10000'}
        shunts = {'A': '10 mΩ', 'mA': '0.5 Ω', 'uA': '47.5 Ω'}

        y = pad
        y += label(pad, y, 'HY3131  %s  mode %s  range %s' % (role, st.get('mode', '-1'), rng), txt, w - 2 * pad)
        y += pad

        # ---- board: jacks -> relays -> networks -> pins ----
        # pins after the datasheet's typical circuit (the DM40 netlist is not known): divider on SENSE and
        # the PA taps, the AC-coupled node on PB4, the Ω/mV network on PB0 / PB3 with Rref between FB and RLU
        # (OP3 drives it through the PA D switches), the shunts into PB1 / PB2, COM on AGND
        pin_v = ['SENSE', 'PA9…5'] + (['PB4'] if role == 'AcV' else [])
        pin_n = ['PB0, PB3', 'FB, RLU']
        pin_c = ['AGND']
        pin_a = ['PB1, PB2']

        def pins(yc, lines, active):
            yy = yc - len(lines) * (L + pad / 2) / 2 + pad / 4
            for s in lines:
                label(x_chip + pad * 0.75, yy, s, txt if active else dim)
                yy += L + pad / 2
        r = L * 0.45
        c_name = pad + 2 * r + pad * 0.75
        name_w = max(tw(n) for n in ('V Ω', 'COM', 'A'))
        pins_w = max(tw(p) for p in pin_v + pin_n + pin_c + pin_a) + 1.5 * pad
        names = dict(rva='PA: V/A', rom='PA: Ω/mV', div='divider ', net='Ω/mV network',
                     sh='shunt %s (%s)' % (shunt, shunts.get(shunt, '')))
        name_x = name_w                        # jack names left of the wires (0: above them)
        for mode_ in ('long', 'short', 'tiny'):
            short = mode_ != 'long'
            if mode_ == 'tiny':
                name_x = 0
            if short:                          # narrow: short box names
                names = dict(rva='V/A', rom='Ω/mV', div='', net='Ω/mV', sh='%s %s' % (shunt, shunts.get(shunt, '')))
            bp = pad * (1 if short else 2)     # box padding
            relay_w = max(tw(names['rva']), tw(names['rom'])) + bp
            net_w = max(tw(names['div'] + (divs.get(rng, '') if rv else '')), tw(names['net'])) + bp
            sh_w = tw(names['sh']) + bp
            base = c_name + name_x + relay_w + net_w + pins_w + pad
            if base + 6 * pad <= w and c_name + name_x + 2 * pad + sh_w + 2 * pad + pins_w + pad <= w:
                break
        notes_below = short                    # narrow: the ohm source notes go under the A row
        sh = names['sh']
        gap = max(pad * (1.2 if short else 2), (w - base) / 3)
        x_relay = c_name + name_x + gap
        x_net = x_relay + relay_w + gap
        x_chip = w - pins_w - pad              # chip's left edge; pin names inside it
        row_h = 2 * L + pad * 2

        yv = y + L * 1.5                       # room for 'COM / AGND' style labels above wires
        rows = [('V Ω', C_RED), ('COM', C_BLACK), ('A', C_RED)]
        y_net = yv + 2 * L + pad * 3
        extra = []
        if rohm:
            extra = ['source %s V (%s)' % (st.get('vsrc', '?'), st.get('vrefsel', '?')), 'Rref %s Ω' % st.get('rref', '?')]
            if 'vx' in st:
                extra.append('node %s V' % st['vx'])
        ext_h = 0
        for s_ in ([] if notes_below else extra):                       # measured like the drawing below (they wrap before the chip)
            pl.set_text(s_, -1)
            pl.set_width(int((x_chip - pad * 2 - x_net) * Pango.SCALE))
            pl.set_wrap(Pango.WrapMode.WORD_CHAR)
            ext_h += pl.get_pixel_size()[1] + pad / 2
            pl.set_width(-1)
        ycom = y_net + L + pad * 2 + ext_h + pad
        ya = ycom + row_h
        y_end = ya + L
        if cr:                                 # the chip, continued below as the 'inside' box
            cr.set_source_rgb(*chipc)
            cr.rectangle(x_chip, yv - L, w - pad - x_chip, y_end - yv + L + pad * 2)
            cr.fill()
        if ink is not None:                    # the chip edge: boxes must keep clear of it
            ink.append(('chip edge', x_chip - 1, yv - L, x_chip, y_end + pad * 2))
        for (name, col), yy in zip(rows, (yv, ycom, ya)):
            if cr:
                cr.new_path()
                cr.set_source_rgb(*col)
                cr.arc(pad + r, yy, r, 0, 6.3)
                cr.fill()
                cr.set_source_rgb(*txt)
                cr.arc(pad + r, yy, r * 0.45, 0, 6.3)
                cr.fill()
            if name_x:
                label(c_name, yy - L / 2, name, txt)
            else:
                label(pad, yy - r - pad / 2 - L, name, txt)
        x_wire = c_name + name_x + (pad / 2 if name_x else 0)
        # V row: relay, then divider or the ohm / mV network
        line(x_wire, yv, x_relay, yv, True)
        box(x_relay, yv, relay_w, names['rva'] if va else names['rom'], True)
        line(x_relay + relay_w, yv, x_net, yv, va)
        line(x_relay + relay_w, yv, x_net, y_net, not va)
        box(x_net, yv, net_w, names['div'] + (divs.get(rng, '') if rv else ''), va and rv)
        box(x_net, y_net, net_w, names['net'], (not va) and (rmv or rohm))
        line(x_net + net_w, yv, x_chip, yv, va and rv)
        line(x_net + net_w, y_net, x_chip, y_net, (not va) and (rmv or rohm))
        pins(yv, pin_v, va and rv)
        pins(y_net, pin_n, (not va) and (rmv or rohm))
        yy = y_net + L / 2 + pad * 1.5
        for s in ([] if notes_below else extra):
            yy += label(x_net, yy, s, hot, x_chip - pad * 2 - x_net) + pad / 2
        # COM
        line(x_wire, ycom, x_chip, ycom, True)
        pins(ycom, pin_c, True)
        # A row: shunt
        line(x_wire, ya, x_relay, ya, True)
        box(x_relay, ya, sh_w, sh, ra)
        line(x_relay + sh_w, ya, x_chip, ya, ra)
        pins(ya, pin_a, ra)
        y = y_end + pad * 3
        if notes_below and extra:
            y += label(pad, y, '   '.join(s.replace(' ', '\u00a0') for s in extra), hot, w - 2 * pad) + pad / 2
        y += label(pad, y, 'V jack %s V    I jack %s A    V shunt %s V' % (
            st.get('vjack', '0'), st.get('ijack', '0'), st.get('vshunt', '0')), hot, w - 2 * pad)
        y += pad / 2
        y += label(pad, y, 'pins: the datasheet\'s typical circuit (the DM40 netlist is not known)', dim, w - 2 * pad)
        y += pad

        # ---- inside the chip: muxes (selected option filled) -> blocks ----
        top = y
        if cr and self.inside_h:
            cr.set_source_rgb(*chipc)
            cr.rectangle(pad, top, w - 2 * pad, self.inside_h)
            cr.fill()
        x0, x1 = pad * 2, w - pad * 2          # content inside the chip box
        y += pad
        y += label(x0, y, 'inside the HY3131', txt)
        y += pad

        def tokens(y, head, active, key, note=''):
            """'head' then the options of mux `key` flowing to the right, the selected one filled."""
            opts = self.MUX[key]
            sel = st.get(key, '')
            if key in ('sad1i', 'sft1'):
                try:
                    sel = opts[int(sel)]
                except (ValueError, IndexError):
                    sel = ''
            hw = tw(head)
            label(x0, y, head, txt if active else off)
            x = x0 + hw + pad
            if x > x1 - tw('SENSE') - pad:
                y += L + pad / 2
                x = x0 + pad * 2
            tx0 = x
            for o in opts:
                ow = tw(o) + pad
                if x + ow > x1:
                    y += L + pad / 2
                    x = tx0
                if o == sel:
                    if cr:
                        cr.set_source_rgb(*(on if active else off))
                        cr.rectangle(x - pad / 2, y - pad / 4, ow, L + pad / 2)
                        cr.fill()
                    label(x, y, o, txt)
                else:
                    label(x, y, o, dim if o not in self.INTERNAL else off)
                x += ow + pad / 2
            if note:
                nw = tw(note)
                if x + nw > x1:
                    y += L + pad / 2
                    x = tx0
                y += label(x, y, note, hot, x1 - x) - L
            return y + L + pad

        def flag(k):
            return st.get(k) == '1'

        op1x = ' x10' if flag('ops1') else ''
        ad1 = flag('ad1')
        cmpn = ''
        if flag('cmp'):
            cmpn = '→ ACPO%s%s' % (' → counter' if flag('ctr') else '', ', CMP pin HIGH' if flag('cmppin') else '')
        blocks = [('OP1' + op1x, 'op1', [('+in', 'sop1p')], ''),
                  ('OP2', 'op2', [('+in', 'sop2p')], ''),
                  ('AD1', 'ad1', [('+in', 'sad1fp'), ('−in', 'sad1fn'), ('ref+', 'sad1rh'), ('ref−', 'sad1rl'),
                                  ('input', 'sad1i'), ('pre-filter', 'sft1')],
                   'OSR %s%s  code %s' % (st.get('osr1', ''), ' chop' if flag('chop') else '', st.get('ad1code', ''))
                   if ad1 else ''),
                  ('AD2', 'ad2', [('+in', 'sad2ip'), ('−in', 'sad2in'), ('ref+', 'sad2rh'), ('ref−', 'sad2rl')], ''),
                  ('AD3', 'ad3', [('+in', 'sad3ip'), ('−in', 'sad3in')], ''),
                  ('CMP', 'cmp', [('in', 'scmpi')], cmpn)]
        if w >= 760:
            # wide (pop-out): every mux with all its options
            for head, en, muxes, note in blocks:
                for i, (pre, key) in enumerate(muxes):
                    y = tokens(y, '%s %s (%s)' % (head, pre, key.upper()), flag(en), key,
                               note if i == len(muxes) - 1 else '')
                y += pad / 2
        else:
            # narrow: one line per block, its selections as filled chips
            for head, en, muxes, note in blocks:
                a_ = flag(en)
                hw = max(tw(b[0]) for b in blocks) + pad * 2
                label(x0, y, head, txt if a_ else off)
                x = x0 + hw
                for pre, key in muxes:
                    sel = st.get(key, '')
                    if key in ('sad1i', 'sft1'):
                        try:
                            sel = self.MUX[key][int(sel)]
                        except (ValueError, IndexError):
                            pass
                    pw_, sw_ = tw(pre), tw(sel)
                    cw_ = pw_ + pad + sw_ + pad
                    if x + cw_ > x1:
                        y += L + pad / 2
                        x = x0 + hw
                    label(x, y, pre, dim)
                    if cr:
                        cr.set_source_rgb(*(on if a_ else off))
                        cr.rectangle(x + pw_ + pad / 2, y - pad / 4, sw_ + pad, L + pad / 2)
                        cr.fill()
                    label(x + pw_ + pad, y, sel, txt)
                    x += cw_ + pad * 1.5
                if note:
                    if x + tw(note) > x1:
                        y += L + pad / 2
                        x = x0 + hw
                    y += label(x, y, note, hot, x1 - x) - L
                y += L + pad
        # blocks after the converters
        y += pad / 2
        items = [('LPF', flag('lpf')), ('RMS', flag('rms')), ('counter', flag('ctr')), ('comparator', flag('cmp')),
                 ('diode /10 (SDIO)', flag('sdio'))]
        sq = L * 0.55
        x = x0
        for s, a in items:
            iw = sq + pad + tw(s) + pad * 3
            if x + iw > x1:
                y += L + pad / 2
                x = x0
            if cr:
                cr.set_source_rgb(*(on if a else off))
                cr.rectangle(x, y + (L - sq) / 2, sq, sq)
                cr.fill()
            label(x + sq + pad, y, s, txt if a else off)
            x += iw
        y += L + pad * 1.5
        # OP3 source and the switch network
        try:
            sm = int(st.get('smode', '0'), 16)
        except ValueError:
            sm = 0
        y += label(x0, y, 'OP3 source: VREF = %s (SFUVR %s)   power bus (SMODE<3:0> = %d): %s%s%s' % (
            st.get('vrefsel', '?'), st.get('sfuvr', '?'), sm & 15, self.SMODE.get(sm & 15, '?'),
            ', RLU' if sm & 0x10 else '', ', cap array' if sm & 0x20 else ''), txt, x1 - x0)
        y += pad
        # PA switch matrix: rows P / D / F / S, columns PA9..PA0, closed = filled
        closed = {}
        for part in st.get('pasw', '-').split(','):
            if part and part != '-' and part[0].isdigit():
                n = int(part.rstrip('PDFS'))
                closed[n] = part[len(str(n)):]
        rows_m = [('P', 'power bus'), ('D', 'OP3 out'), ('F', 'FB'), ('S', 'SENSE')]
        if w < 760:
            sw = '  '.join('PA%d %s' % (n, closed[n]) for n in sorted(closed, reverse=True)) or 'all open'
            y += label(x0, y, 'PA switches (P power bus, D OP3 out, F FB, S SENSE): ' + sw, txt, x1 - x0)
            y += pad
            rows_m = []
        rl_w = max([tw(n) for _, n in rows_m] or [0]) + pad
        cw = max(tw('PA9'), L) + pad
        if x0 + rl_w + 10 * cw > x1:           # narrow: shorter row names
            rows_m = [(k, k) for k, _ in rows_m]
            rl_w = tw('P') + pad * 2
        if rows_m:
            for i in range(10):
                label(x0 + rl_w + i * cw + (cw - tw('PA%d' % (9 - i))) / 2, y, 'PA%d' % (9 - i), dim)
            y += L + pad / 2
        dot = L * 0.45
        for k, name in rows_m:
            label(x0, y, name, dim)
            for i in range(10):
                n = 9 - i
                cx = x0 + rl_w + i * cw + cw / 2
                if cr:
                    cr.new_path()
                    cr.arc(cx, y + L / 2, dot / 2, 0, 6.3)
                    if k in closed.get(n, ''):
                        cr.set_source_rgb(*on)
                        cr.fill()
                    else:
                        cr.set_source_rgb(*off)
                        cr.set_line_width(1)
                        cr.stroke()
                        cr.set_line_width(max(1.5, L / 9))
                if ink is not None:
                    ink.append(('dot', cx - dot / 2, y + (L - dot) / 2, cx + dot / 2, y + (L + dot) / 2))
            y += L + pad / 2
        y += pad
        y += label(x0, y, 'period %s s   die %s °C   %s' % (st.get('period', ''), st.get('die', ''),
                                                          'LPF %s  RMS %s' % (st.get('lpfcode', ''), st.get('rmscode', ''))
                                                          if flag('ad2') else ''), txt, x1 - x0)
        y += pad
        if cr is None:
            self.inside_h = y - top
        return y + pad


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

        self.panel = FrontPanel(self.on_panel_event)
        content = Gtk.Box(orientation=Gtk.Orientation.VERTICAL)
        content.append(header)
        # The device picture at an integer scale top-left; the input cards fill the rest: DMM jacks and
        # the front-end schematic in the right column, scope / generator+USB / monitor under the device
        # when there is room there (else at the end of the right column, which scrolls).
        body = Gtk.Box(vexpand=True)
        left = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, valign=Gtk.Align.START)
        left.set_hexpand(False)               # not inherited from the cards under the device
        left.append(self.panel)
        self.bottom = Gtk.Box(spacing=CARD_GAP, margin_start=PANEL_MARGIN,
                              margin_end=PANEL_MARGIN, margin_bottom=CARD_GAP)
        # a non-scrolling ScrolledWindow: natural width = minimum width, so the cards never widen the
        # column beyond the device picture
        self.bottom_wrap = Gtk.ScrolledWindow(hscrollbar_policy=Gtk.PolicyType.NEVER,
                                              vscrollbar_policy=Gtk.PolicyType.NEVER, propagate_natural_height=True)
        self.bottom_wrap.set_child(self.bottom)
        left.append(self.bottom_wrap)
        body.append(left)
        self.right = Gtk.Box(orientation=Gtk.Orientation.VERTICAL, spacing=CARD_GAP, margin_start=CARD_GAP // 2,
                             margin_end=CARD_GAP, margin_top=CARD_GAP, margin_bottom=CARD_GAP)
        scroll = Gtk.ScrolledWindow(hexpand=True, vexpand=True, hscrollbar_policy=Gtk.PolicyType.NEVER)
        scroll.set_size_request(SIDE_MIN_W, -1)
        scroll.set_child(self.right)
        body.append(scroll)
        self.cards = self.build_cards()
        self.right.append(self.cards['dmm'])
        self.right.append(self.cards['fe'])
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
        outer, box = self.card('Front end, HY3131', big)
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
        """Window size changed: the device's integer scale, and where the small cards go."""
        W, H = body.get_width(), body.get_height()
        if W <= 0 or (W, H) == self.body_size:
            return True
        self.body_size = (W, H)
        sf = self.win.get_scale_factor()
        pw1 = DEV_W * PX_PER_MM_1X / sf + 2 * PANEL_MARGIN
        ph1 = PANEL_H * PX_PER_MM_1X / sf + 2 * PANEL_MARGIN
        k = max(1, int(min((W - SIDE_MIN_W) / pw1, H / ph1)))
        if k != self.panel.want_k:
            self.panel.want_k = k
            self.panel.queue_resize()
        # the small cards go under the device when they fit there, else into the right column
        pw = DEV_W * PX_PER_MM_1X * k / sf
        each = (pw - 2 * CARD_GAP) / len(self.bottom_cards)
        need_h = max(c.measure(Gtk.Orientation.VERTICAL, int(each))[1] for c in self.bottom_cards)
        need_w = sum(c.measure(Gtk.Orientation.HORIZONTAL, -1)[0] for c in self.bottom_cards)
        below = H - PANEL_H * PX_PER_MM_1X * k / sf - 2 * PANEL_MARGIN - CARD_GAP
        want = below >= need_h and pw >= need_w + 2 * CARD_GAP
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
