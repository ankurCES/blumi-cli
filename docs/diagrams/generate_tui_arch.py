#!/usr/bin/env python3
"""Generate docs/diagrams/tui-architecture.png — a detailed component diagram of
the blumi TUI: ratatui (MVU) <-> blumi-core session actor <-> tools/runtime <->
grid, with a numbered request (Command) / response (Event) process flow."""
import math, os
from PIL import Image, ImageDraw, ImageFont

OUT = os.path.dirname(__file__)
W, H = 1480, 920
S = 2
WW, HH = W * S, H * S

BG=(14,17,22); BG2=(20,24,31); PANEL=(26,31,40); PANEL2=(31,37,48)
STROKE=(54,60,74); PINK=(255,79,135); PURPLE=(107,80,255); PURPLE2=(155,134,255)
MINT=(104,255,214); MUTED=(139,143,163); TEXT=(238,236,244)
MACBLUE=(110,168,254); AMBER=(245,162,93); DIM=(40,46,58)

SANS="/System/Library/Fonts/Supplemental/Arial.ttf"
SANSB="/System/Library/Fonts/Supplemental/Arial Bold.ttf"
MONO="/System/Library/Fonts/Menlo.ttc"
def F(p,s): return ImageFont.truetype(p,int(s*S))
f_title=F(SANSB,26); f_h=F(SANSB,17); f_b=F(SANS,13); f_m=F(MONO,12)
f_badge=F(SANSB,14); f_tag=F(SANS,11); f_lane=F(SANSB,13); f_sub=F(SANS,13)

def R(v): return int(round(v*S))
def rr(d,x,y,w,h,r,fill=None,outline=None,width=1):
    d.rounded_rectangle([R(x),R(y),R(x+w),R(y+h)],radius=R(r),fill=fill,outline=outline,width=R(width))
def tc(d,x,y,s,f,fill,anchor="mm"):
    d.text((R(x),R(y)),s,font=f,fill=fill,anchor=anchor)
def circ(d,cx,cy,rad,fill=None,outline=None,width=1):
    d.ellipse([R(cx-rad),R(cy-rad),R(cx+rad),R(cy+rad)],fill=fill,outline=outline,width=R(width))
def mix(a,b,t): return tuple(int(a[i]+(b[i]-a[i])*t) for i in range(3))

img=Image.new("RGB",(WW,HH),BG); d=ImageDraw.Draw(img)
rr(d,16,16,W-32,H-32,18,fill=BG2,outline=STROKE,width=1.5)

# flower
def flower(cx,cy,s,c):
    for a in range(0,360,72):
        r=math.radians(a); circ(d,cx+math.cos(r)*s*0.3,cy+math.sin(r)*s*0.3,s*0.26,fill=c)
    circ(d,cx,cy,s*0.2,fill=BG2)
flower(46,46,18,PINK)
tc(d,68,40,"blumi — TUI session architecture",f_title,TEXT,anchor="lm")
tc(d,68,64,"one UI-agnostic core, one event stream — the same flow drives the web UI and the blugo phone app",f_sub,MUTED,anchor="lm")

# ---- column panels ----
PY, PH = 150, 560
cols = [
    ("blumi-tui", "ratatui · MVU", 44, 318, PINK,
     [("Explorer rail","workspaces · sessions · skills · files"),
      ("Transcript","streaming markdown · tool cards"),
      ("Editor","slash commands · keybindings"),
      ("Agent rail","active agents · cost · context"),
      ("/remote attach","live-watch a gateway over SSE")]),
    ("blumi-core", "session actor", 410, 318, PURPLE2,
     [("Agent loop","turn runner · streaming"),
      ("Context manager","window · compaction · memory"),
      ("Permission engine","approvals · YOLO"),
      ("Event / Command bus","typed, UI-agnostic")]),
    ("tools & runtime", "blumi-tools · exec", 776, 318, MINT,
     [("Core tools","read · write · edit · bash · search"),
      ("delegate","spawn sub-agents (cap 4)"),
      ("grid_dispatch","run a prompt on a peer"),
      ("executor","Local · Docker · SSH")]),
    ("grid", "blumi · grid", 1142, 296, AMBER,
     [("grid client","HTTP+SSE · X-Blumi-Grid"),
      ("peer registry","mDNS + static peers"),
      ("→ peers","ankur-mac-air · predator-blum")]),
]
col_x = {}
for name,sub,x,w,accent,items in cols:
    rr(d,x,PY,w,PH,16,fill=PANEL,outline=mix(STROKE,accent,0.5),width=2)
    rr(d,x,PY,w,40,16,fill=mix(PANEL,accent,0.16))
    rr(d,x,PY+26,w,16,0,fill=PANEL)  # square off header bottom
    tc(d,x+16,PY+20,name,f_h,TEXT,anchor="lm")
    tc(d,x+w-14,PY+20,sub,f_tag,mix(TEXT,accent,0.5),anchor="rm")
    yy=PY+58
    for t,s in items:
        bh=72 if len(items)<=4 else 58
        rr(d,x+12,yy,w-24,bh-10,9,fill=PANEL2,outline=STROKE,width=1)
        tc(d,x+24,yy+ (16 if s else bh/2-5),t,f_b,mix(TEXT,accent,0.25),anchor="lm")
        if s: tc(d,x+24,yy+36,s,f_m,MUTED,anchor="lm")
        yy+=bh
    col_x[name]=(x,w,accent)

# side attachments to core/tools
def chip(cx,cy,w,h,label,sub,accent):
    rr(d,cx-w/2,cy-h/2,w,h,11,fill=PANEL,outline=mix(STROKE,accent,0.6),width=2)
    tc(d,cx,cy-8,label,f_b,TEXT); tc(d,cx,cy+12,sub,f_tag,MUTED)
chip(935,98,250,52,"LLM provider","Claude · OpenAI · Gemini · Azure (BYOK)",PURPLE)
chip(935,H-92,250,52,"SQLite store","sessions · checkpoints · FTS5 search",MINT)
# dashed connectors
def dash(x0,y0,x1,y1,c,seg=10):
    n=int(math.hypot(x1-x0,y1-y0)/seg)
    for i in range(n):
        if i%2: continue
        a,b=i/n,(i+1)/n
        d.line([(R(x0+(x1-x0)*a),R(y0+(y1-y0)*a)),(R(x0+(x1-x0)*b),R(y0+(y1-y0)*b))],fill=c,width=R(1.6))
dash(935,124,935,PY,mix(BG2,PURPLE,0.6))
dash(935,H-118,935,PY+PH,mix(BG2,MINT,0.6))

# ---- flow lanes ----
def arrow(x0,y0,x1,y1,c,wd=3):
    d.line([(R(x0),R(y0)),(R(x1),R(y1))],fill=c,width=R(wd))
    a=math.atan2(y1-y0,x1-x0)
    for da in (math.radians(150),math.radians(-150)):
        d.line([(R(x1),R(y1)),(R(x1+math.cos(a+da)*11),R(y1+math.sin(a+da)*11))],fill=c,width=R(wd))
def badge(x,y,n,c):
    circ(d,x,y,12,fill=c,outline=BG2,width=2); tc(d,x,y-1,n,f_badge,(12,14,18))

xs=[44+318, 410, 410+318, 776, 776+318, 1142]  # column edges
# REQUEST lane (top, pink): You -> tui -> core -> tools -> grid -> peer
ry=PY-26
tc(d,40,ry-20,"▶  request  ·  Command",f_lane,PINK,anchor="lm")
# You node
rr(d,44,ry-12,0,0,0)  # noop
arrow(362,ry,410,ry,PINK); badge(386,ry,"1",PINK)            # tui->core
arrow(728,ry,776,ry,PINK); badge(752,ry,"2",PINK)            # core->tools
arrow(1094,ry,1142,ry,PINK); badge(1118,ry,"3",PINK)         # tools->grid
tc(d,150,ry,"keystroke → editor",f_m,mix(TEXT,PINK,0.3))
tc(d,540,ry,"UserMessage",f_m,mix(TEXT,PINK,0.3))
tc(d,906,ry,"tool call",f_m,mix(TEXT,PINK,0.3))
tc(d,1300,ry,"grid_dispatch",f_m,mix(TEXT,PINK,0.3))

# RESPONSE lane (bottom, mint): peer -> tools -> core -> tui (re-render)
yy=PY+PH+30
tc(d,40,yy+22,"◀  response  ·  Event stream (re-render)",f_lane,MINT,anchor="lm")
arrow(1142,yy,1094,yy,MINT); badge(1118,yy,"4",MINT)
arrow(776,yy,728,yy,MINT);  badge(752,yy,"5",MINT)
arrow(410,yy,362,yy,MINT);  badge(386,yy,"6",MINT)
tc(d,1300,yy,"peer result",f_m,mix(TEXT,MINT,0.35))
tc(d,906,yy,"ToolResult",f_m,mix(TEXT,MINT,0.35))
tc(d,540,yy,"Token · TurnDone",f_m,mix(TEXT,MINT,0.35))
tc(d,150,yy,"view re-renders",f_m,mix(TEXT,MINT,0.35))

# peers mini-strip under grid
for i,(nm,c) in enumerate((("ankur-mac-air",MACBLUE),("predator-blum",AMBER))):
    px=1142+ i*0; py=PY+PH-92+ i*0
# (peers already named in the grid column)

img.save(os.path.join(OUT,"tui-architecture.png"))
print("wrote tui-architecture.png", os.path.getsize(os.path.join(OUT,"tui-architecture.png"))//1024,"KB")
