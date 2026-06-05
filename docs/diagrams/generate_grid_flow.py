#!/usr/bin/env python3
"""Generate docs/diagrams/grid-flow.gif — an animated network-flow diagram of
blumi grid task execution: blugo (phone) -> origin gateway -> fan out to peers ->
results return. Brand palette from assets/blumi-logo.svg. Pure Pillow."""
import math
import os
from PIL import Image, ImageDraw, ImageFont

OUT = os.path.join(os.path.dirname(__file__))
W, H = 1200, 660
S = 2                      # supersample for anti-aliasing
WW, HH = W * S, H * S

# --- brand palette -------------------------------------------------------
BG      = (14, 17, 22)
BG2     = (20, 24, 31)
PANEL   = (26, 31, 40)
STROKE  = (54, 60, 74)
PINK    = (255, 79, 135)
PURPLE  = (107, 80, 255)
PURPLE2 = (155, 134, 255)
MINT    = (104, 255, 214)
MUTED   = (139, 143, 163)
TEXT    = (238, 236, 244)
MACBLUE = (110, 168, 254)
AMBER   = (245, 162, 93)
DIM     = (38, 43, 54)

SANS  = "/System/Library/Fonts/Supplemental/Arial.ttf"
SANSB = "/System/Library/Fonts/Supplemental/Arial Bold.ttf"
MONO  = "/System/Library/Fonts/Menlo.ttc"

def F(path, sz):
    return ImageFont.truetype(path, int(sz * S))

f_title = F(SANSB, 24)
f_node  = F(SANSB, 18)
f_sub   = F(SANS, 13)
f_mono  = F(MONO, 12)
f_cap   = F(SANSB, 19)
f_step  = F(SANSB, 14)
f_tag   = F(SANS, 11)

def R(v): return int(round(v * S))
def lerp(a, b, t): return a + (b - a) * t
def smooth(t):
    t = max(0.0, min(1.0, t)); return t * t * (3 - 2 * t)
def mix(c1, c2, t):
    return tuple(int(round(lerp(c1[i], c2[i], t))) for i in range(3))

def rrect(d, x, y, w, h, r, fill=None, outline=None, width=1):
    d.rounded_rectangle([R(x), R(y), R(x + w), R(y + h)], radius=R(r),
                        fill=fill, outline=outline, width=R(width))

def rrect_xy(d, x0, y0, x1, y1, r, **kw):
    rrect(d, x0, y0, x1 - x0, y1 - y0, r, **kw)

def textc(d, x, y, s, font, fill, anchor="mm"):
    d.text((R(x), R(y)), s, font=font, fill=fill, anchor=anchor)

def circle(d, cx, cy, rad, fill=None, outline=None, width=1):
    d.ellipse([R(cx - rad), R(cy - rad), R(cx + rad), R(cy + rad)],
              fill=fill, outline=outline, width=R(width))

def glow_dot(base, cx, cy, rad, color):
    """Soft glowing token via an RGBA overlay composited on `base`."""
    ov = Image.new("RGBA", base.size, (0, 0, 0, 0))
    od = ImageDraw.Draw(ov)
    for i, a in ((3.2, 40), (2.2, 70), (1.5, 120)):
        od.ellipse([R(cx - rad * i), R(cy - rad * i), R(cx + rad * i), R(cy + rad * i)],
                   fill=color + (a,))
    od.ellipse([R(cx - rad), R(cy - rad), R(cx + rad), R(cy + rad)], fill=color + (255,))
    od.ellipse([R(cx - rad * .45), R(cy - rad * .45), R(cx + rad * .45), R(cy + rad * .45)],
               fill=(255, 255, 255, 230))
    base.alpha_composite(ov)

# --- icons ---------------------------------------------------------------
def icon_flower(d, cx, cy, s, color):
    r = s * 0.30
    for ang in range(0, 360, 72):
        a = math.radians(ang)
        circle(d, cx + math.cos(a) * r, cy + math.sin(a) * r, s * 0.26, fill=color)
    circle(d, cx, cy, s * 0.20, fill=BG)

def icon_phone(d, cx, cy, accent):
    w, h = 58, 104
    rrect(d, cx - w/2, cy - h/2, w, h, 12, fill=(18, 21, 28), outline=accent, width=2.5)
    rrect(d, cx - w/2 + 6, cy - h/2 + 12, w - 12, h - 24, 5, fill=(11, 13, 18))
    d.line([(R(cx - 8), R(cy - h/2 + 6)), (R(cx + 8), R(cy - h/2 + 6))], fill=STROKE, width=R(2))
    icon_flower(d, cx, cy - 14, 20, accent)
    # little result rows
    for i, yy in enumerate((cy + 16, cy + 30)):
        rrect(d, cx - 18, yy, 36, 7, 3, fill=mix(BG, accent, 0.5))

def icon_terminal(d, cx, cy, accent, star=False):
    w, h = 92, 70
    rrect(d, cx - w/2, cy - h/2, w, h, 9, fill=(12, 15, 20), outline=accent, width=2.5)
    rrect(d, cx - w/2, cy - h/2, w, 16, 9, fill=mix(BG, accent, 0.18))
    for i, dotc in enumerate((PINK, AMBER, MINT)):
        circle(d, cx - w/2 + 12 + i * 11, cy - h/2 + 8, 3.2, fill=dotc)
    textc(d, cx - w/2 + 14, cy + 6, ">_", F(MONO, 16), accent, anchor="lm")
    if star:
        circle(d, cx + w/2 - 4, cy - h/2 - 2, 11, fill=PINK)
        textc(d, cx + w/2 - 4, cy - h/2 - 3, "★", F(SANSB, 12), (255, 255, 255))

def icon_server(d, cx, cy, accent, online=True):
    w, h = 80, 78
    rrect(d, cx - w/2, cy - h/2, w, h, 8, fill=(13, 16, 22), outline=accent, width=2.5)
    for i in range(3):
        yy = cy - h/2 + 12 + i * 20
        rrect(d, cx - w/2 + 8, yy, w - 16, 13, 3, fill=mix(BG, accent, 0.16))
        circle(d, cx - w/2 + 16, yy + 6.5, 3.2, fill=(MINT if online else MUTED))
        rrect(d, cx - w/2 + 26, yy + 4.5, w - 42, 4, 2, fill=mix(BG, accent, 0.4))

# --- node geometry -------------------------------------------------------
PHONE  = (140, H/2)
ORIGIN = (560, H/2)
PEER1  = (1018, 232)   # mac-air
PEER2  = (1018, 470)   # predator

def draw_node_box(d, cx, cy, w, h, accent, glow=0.0):
    col = mix(STROKE, accent, glow)
    rrect(d, cx - w/2, cy - h/2, w, h, 14,
          fill=mix(PANEL, accent, 0.05 + glow * 0.05),
          outline=col, width=2 + glow * 1.5)

def edge(d, p0, p1, active=0.0, dash=False):
    col = mix(DIM, PINK, active)
    w = 2 + active * 1.6
    x0, y0 = p0; x1, y1 = p1
    if dash:
        n = 26
        for i in range(n):
            if i % 2: continue
            a, b = i / n, (i + 1) / n
            d.line([(R(lerp(x0, x1, a)), R(lerp(y0, y1, a))),
                    (R(lerp(x0, x1, b)), R(lerp(y0, y1, b)))], fill=col, width=R(w))
    else:
        d.line([(R(x0), R(y0)), (R(x1), R(y1))], fill=col, width=R(w))

def arrowhead(d, p0, p1, color, size=9):
    x0, y0 = p0; x1, y1 = p1
    a = math.atan2(y1 - y0, x1 - x0)
    for da in (math.radians(150), math.radians(-150)):
        d.line([(R(x1), R(y1)),
                (R(x1 + math.cos(a + da) * size), R(y1 + math.sin(a + da) * size))],
               fill=color, width=R(2.4))

# ports (connection points on node edges)
PH_R = (PHONE[0] + 75, PHONE[1])
OR_L = (ORIGIN[0] - 98, ORIGIN[1])
OR_R = (ORIGIN[0] + 98, ORIGIN[1])
P1_L = (PEER1[0] - 85, PEER1[1])
P2_L = (PEER2[0] - 85, PEER2[1])

CAPTIONS = [
    ("1", "Send a task from blugo", "POST /api/grid/delegate", PINK),
    ("2", "Orchestrator fans it across the grid", "/api/grid/run -> every live peer", PURPLE2),
    ("3", "Each machine runs its share", "real turn on its own runtime", MINT),
    ("4", "Results return — tagged by machine", "hostname - output - latency", PINK),
]

N = 52
def phase_for(i):
    # returns (phase_index 0..3, local_t 0..1)
    segs = [(0, 12), (12, 24), (24, 32), (32, 44), (44, 52)]
    # map 5 timeline segments onto 4 captions (compute shares caption 3? keep 0..3)
    return segs

def build_frame(i):
    img = Image.new("RGBA", (WW, HH), BG + (255,))
    d = ImageDraw.Draw(img)
    # bg vignette panel
    rrect(d, 16, 16, W - 32, H - 32, 18, fill=BG2, outline=STROKE, width=1.5)
    # title
    icon_flower(d, 44, 44, 18, PINK)
    textc(d, 66, 44, "blumi grid", f_title, TEXT, anchor="lm")
    textc(d, 66, 66, "distributed task execution across your LAN", f_sub, MUTED, anchor="lm")

    # timeline -> which edge is active
    # segments: send(0-12) fan(12-24) compute(24-32) return(32-44) deliver(44-52)
    def seg(a, b): return (i >= a and i < b, (i - a) / max(1, (b - a)))
    s_send, t_send = seg(0, 12)
    s_fan,  t_fan  = seg(12, 24)
    s_comp, t_comp = seg(24, 32)
    s_ret,  t_ret  = seg(32, 44)
    s_del,  t_del  = seg(44, 52)

    a_send = 1.0 if s_send else 0.0
    a_fan  = 1.0 if s_fan else 0.0
    a_ret  = 1.0 if s_ret else 0.0
    a_del  = 1.0 if s_del else 0.0
    peer_hot = 1.0 if (s_comp or s_fan) else 0.0

    # edges (under nodes)
    edge(d, PH_R, OR_L, active=max(a_send, a_del), dash=True)
    edge(d, OR_R, P1_L, active=max(a_fan, a_ret))
    edge(d, OR_R, P2_L, active=max(a_fan, a_ret))
    arrowhead(d, PH_R, OR_L, mix(DIM, PINK, max(a_send, a_del)))
    arrowhead(d, OR_R, P1_L, mix(DIM, PINK, max(a_fan, a_ret)))
    arrowhead(d, OR_R, P2_L, mix(DIM, PINK, max(a_fan, a_ret)))

    # nodes
    draw_node_box(d, PHONE[0], PHONE[1], 150, 200, PINK, glow=(a_send + a_del) * 0.8)
    icon_phone(d, PHONE[0], PHONE[1] - 14, PINK)
    textc(d, PHONE[0], PHONE[1] + 70, "blugo", f_node, TEXT)
    textc(d, PHONE[0], PHONE[1] + 90, "companion app", f_tag, MUTED)

    draw_node_box(d, ORIGIN[0], ORIGIN[1], 196, 168, PURPLE2,
                  glow=max(a_send, a_fan, a_ret, a_del) * 0.7)
    icon_terminal(d, ORIGIN[0], ORIGIN[1] - 14, PURPLE2, star=True)
    textc(d, ORIGIN[0], ORIGIN[1] + 40, "blumi serve - orchestrator", f_node, TEXT)
    textc(d, ORIGIN[0], ORIGIN[1] + 60, "owns the board - TUI / web / phone", f_tag, MUTED)

    draw_node_box(d, PEER1[0], PEER1[1], 170, 156, MACBLUE, glow=peer_hot * 0.85)
    icon_server(d, PEER1[0], PEER1[1] - 18, MACBLUE)
    textc(d, PEER1[0], PEER1[1] + 42, "ankur-mac-air", f_node, TEXT)
    textc(d, PEER1[0], PEER1[1] + 62, "macOS - Apple Silicon", f_tag, MACBLUE)

    draw_node_box(d, PEER2[0], PEER2[1], 170, 156, AMBER, glow=peer_hot * 0.85)
    icon_server(d, PEER2[0], PEER2[1] - 18, AMBER)
    textc(d, PEER2[0], PEER2[1] + 42, "predator-blum", f_node, TEXT)
    textc(d, PEER2[0], PEER2[1] + 62, "Linux - x86_64", f_tag, AMBER)

    # moving tokens
    if s_send:
        t = smooth(t_send)
        glow_dot(img, lerp(PH_R[0], OR_L[0], t), lerp(PH_R[1], OR_L[1], t), 9, PINK)
    if s_fan:
        t = smooth(t_fan)
        glow_dot(img, lerp(OR_R[0], P1_L[0], t), lerp(OR_R[1], P1_L[1], t), 8, PURPLE2)
        glow_dot(img, lerp(OR_R[0], P2_L[0], t), lerp(OR_R[1], P2_L[1], t), 8, PURPLE2)
    if s_comp:
        # pulsing "running" rings on peers
        pr = 14 + 8 * math.sin(t_comp * math.pi)
        for (px, py, c) in ((PEER1[0]-58, PEER1[1]-8, MACBLUE), (PEER2[0]-58, PEER2[1]-8, AMBER)):
            circle(d, PEER1[0], PEER1[1], 0, fill=None)  # noop keep
        for peer, c in ((PEER1, MACBLUE), (PEER2, AMBER)):
            circle(d, peer[0], peer[1], pr + 60, outline=mix(BG2, c, 0.5), width=2)
    if s_ret:
        t = smooth(t_ret)
        glow_dot(img, lerp(P1_L[0], OR_R[0], t), lerp(P1_L[1], OR_R[1], t), 8, MINT)
        glow_dot(img, lerp(P2_L[0], OR_R[0], t), lerp(P2_L[1], OR_R[1], t), 8, MINT)
    if s_del:
        t = smooth(t_del)
        glow_dot(img, lerp(OR_L[0], PH_R[0], t), lerp(OR_L[1], PH_R[1], t), 9, MINT)

    # caption / step bar
    step = 0 if s_send else 1 if (s_fan) else 2 if s_comp else 2 if s_ret else 3
    num, title, sub, col = CAPTIONS[step]
    by = H - 54
    rrect(d, 30, by - 16, W - 60, 50, 12, fill=PANEL, outline=mix(STROKE, col, 0.5), width=1.5)
    circle(d, 56, by + 9, 15, fill=col)
    textc(d, 56, by + 8, num, f_step, (12, 14, 18))
    textc(d, 82, by, title, f_cap, TEXT, anchor="lm")
    textc(d, 82, by + 20, sub, f_mono, MUTED, anchor="lm")
    # step dots
    for k in range(4):
        cx = W - 60 - (3 - k) * 26
        circle(d, cx, by + 9, 6, fill=(col if k == step else DIM))

    return img.convert("RGB").resize((W, H), Image.LANCZOS)

frames = [build_frame(i) for i in range(N)]
# hold the final delivered frame a touch longer
frames += [frames[-1]] * 3
gif = os.path.join(OUT, "grid-flow.gif")
frames[0].save(gif, save_all=True, append_images=frames[1:], duration=95,
               loop=0, optimize=True, disposal=2)
# a representative still (the fan-out moment) for non-animated contexts
build_frame(18).save(os.path.join(OUT, "grid-flow.png"))
print("wrote", gif, os.path.getsize(gif) // 1024, "KB")
print("wrote grid-flow.png")
