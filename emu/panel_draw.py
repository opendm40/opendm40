"""The DM40 front panel drawn with Cairo, shared by the emulator window
(gtkpanel.FrontPanel, GTK) and headless renders (firmware/tools/ui-render/
manual-annot.py: the device pictures of the user manual).

Geometry is in millimetres of the real device (140 x 83 mm, manual figure
2.1.1): a 480x320 display of 73.5 x 49 mm in a black glass with the
MENU/RUN/AUTO/MF touch strip on its left, power button on the left edge,
V-Ohm / COM / A banana jacks on the right (positions, colours and legends from
the manual figure and product photos). The top edge (USB-C with the charge LED,
MCX generator OUT, MCX scope OSC) is a separate top view above the front view,
like an engineering drawing (y < 0 in the same mm frame). Needs only cairo.
"""
from math import cos, sin

import cairo

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

class PanelArt:
    """Cairo drawing of the panel in mm (cr already scaled). The state the
    drawing reads: pressed_key / pressed_power (highlights), model (the
    printed letter), ink (a list: record ink boxes), show_top (the top view)."""
    pressed_key = None
    pressed_power = False
    model = 'A'
    ink = None
    show_top = SHOW_TOP_VIEW

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
        if self.show_top:
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
