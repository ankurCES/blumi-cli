#!/usr/bin/env python3
"""Generate docs/social-preview.png — a 1280x640 GitHub social card: a collage of
the desk shot + phone Grid tab + TUI, with the blumi flower logo, wordmark, and a
tagline in the foreground. Pure Pillow."""
import math, os
from PIL import Image, ImageDraw, ImageFont, ImageFilter

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
SH = os.path.join(ROOT, "docs", "screenshots")
DG = os.path.join(ROOT, "docs", "diagrams")
W, H = 1280, 640
S = 2
WW, HH = W * S, H * S

BG=(13,16,21); BG2=(19,23,30); PANEL=(26,31,40); STROKE=(60,66,82)
PINK=(255,79,135); PURPLE=(107,80,255); PURPLE2=(165,146,255); MINT=(104,255,214)
MUTED=(166,170,188); TEXT=(243,241,247)
SANS="/System/Library/Fonts/Supplemental/Arial.ttf"
SANSB="/System/Library/Fonts/Supplemental/Arial Bold.ttf"
def F(p,s): return ImageFont.truetype(p,int(s*S))
def R(v): return int(round(v*S))
def mix(a,b,t): return tuple(int(a[i]+(b[i]-a[i])*t) for i in range(3))

base = Image.new("RGB",(WW,HH),BG)

# --- background glow -----------------------------------------------------
glow = Image.new("RGBA",(WW,HH),(0,0,0,0)); gd=ImageDraw.Draw(glow)
gd.ellipse([R(820),R(-160),R(1500),R(420)],fill=PINK+(60,))
gd.ellipse([R(680),R(260),R(1320),R(820)],fill=PURPLE+(45,))
gd.ellipse([R(-200),R(300),R(360),R(820)],fill=PURPLE+(30,))
glow = glow.filter(ImageFilter.GaussianBlur(R(120)))
base = Image.alpha_composite(base.convert("RGBA"), glow)
d = ImageDraw.Draw(base)

# faint dotted grid on the left text zone
for x in range(40, 600, 34):
    for y in range(60, 600, 34):
        d.ellipse([R(x),R(y),R(x+1.5),R(y+1.5)],fill=(255,255,255,14))

def rounded_mask(w,h,r):
    m=Image.new("L",(w,h),0); md=ImageDraw.Draw(m)
    md.rounded_rectangle([0,0,w-1,h-1],radius=r,fill=255); return m

def cover(im, bw, bh):
    iw,ih=im.size; s=max(bw/iw, bh/ih)
    im=im.resize((int(iw*s)+1,int(ih*s)+1),Image.LANCZOS)
    iw,ih=im.size; l=(iw-bw)//2; t=(ih-bh)//2
    return im.crop((l,t,l+bw,t+bh))

def card(path, x, y, w, h, r=18, border=PINK, bw=3, rot=0, shadow=True):
    x,y,w,h=R(x),R(y),R(w),R(h); r=R(r)
    im=Image.open(path).convert("RGB")
    im=cover(im, w, h)
    msk=rounded_mask(w,h,r)
    # border
    bd=Image.new("RGBA",(w,h),(0,0,0,0)); ImageDraw.Draw(bd).rounded_rectangle(
        [0,0,w-1,h-1],radius=r,outline=border+(255,),width=R(bw))
    canvasW=Image.new("RGBA",(w,h),(0,0,0,0)); canvasW.paste(im,(0,0),msk)
    canvasW=Image.alpha_composite(canvasW,bd)
    if rot: canvasW=canvasW.rotate(rot,expand=True,resample=Image.BICUBIC)
    if shadow:
        sh=Image.new("RGBA",base.size,(0,0,0,0))
        ImageDraw.Draw(sh).rounded_rectangle([x+R(8),y+R(14),x+canvasW.size[0]+R(8),y+canvasW.size[1]+R(14)],
                                             radius=r,fill=(0,0,0,150))
        sh=sh.filter(ImageFilter.GaussianBlur(R(14)))
        base.alpha_composite(sh)
    base.alpha_composite(canvasW,(x,y))

# --- collage (right zone) : TUI behind, desk mid, phone front ------------
card(os.path.join(SH,"grid-tui-orchestrator.png"), 668, 70, 588, 250, r=14, border=mix(STROKE,MINT,0.6), bw=2, rot=0)
card(os.path.join(SH,"grid-desk.jpg"),            600, 250, 470, 320, r=16, border=mix(STROKE,PURPLE2,0.7), bw=3, rot=-3)
card(os.path.join(SH,"grid-delegate-tab.jpg"),    1010, 150, 210, 430, r=26, border=PINK, bw=3, rot=4)

d = ImageDraw.Draw(base)  # refresh

# --- foreground: logo + wordmark + tagline (left) ------------------------
def flower(cx,cy,s,c):
    for a in range(0,360,72):
        r=math.radians(a)
        d.ellipse([R(cx+math.cos(r)*s*.32-s*.28),R(cy+math.sin(r)*s*.32-s*.28),
                   R(cx+math.cos(r)*s*.32+s*.28),R(cy+math.sin(r)*s*.32+s*.28)],fill=c+(255,))
    d.ellipse([R(cx-s*.2),R(cy-s*.2),R(cx+s*.2),R(cy+s*.2)],fill=BG+(255,))

LX=66
flower(LX+26, 150, 46, PINK)
d.text((R(LX+62),R(150)),"blumi",font=F(SANSB,72),fill=PINK+(255,),anchor="lm")

d.text((R(LX),R(250)),"One agent. Every machine you own.",font=F(SANSB,33),fill=TEXT+(255,),anchor="lm")
d.text((R(LX),R(298)),"Local-first, provider-agnostic agentic coding —",font=F(SANS,20),fill=MUTED+(255,),anchor="lm")
d.text((R(LX),R(326)),"terminal · web · phone · a distributed LAN grid.",font=F(SANS,20),fill=MUTED+(255,),anchor="lm")

# chips
chips=[("Rust",PURPLE2),("MCP",MINT),("BYOK",PINK),("Apache-2.0",MUTED)]
cx=LX
for label,c in chips:
    fnt=F(SANSB,16); tw=d.textlength(label,font=fnt)/S
    w=tw+30
    d.rounded_rectangle([R(cx),R(372),R(cx+w),R(372+34)],radius=R(17),
                        fill=PANEL+(255,),outline=mix(STROKE,c,0.7)+(255,),width=R(1.5))
    d.text((R(cx+w/2),R(372+16)),label,font=fnt,fill=mix(TEXT,c,0.4)+(255,),anchor="mm")
    cx+=w+12

# footer url
d.text((R(LX),R(560)),"github.com/ankurCES/blumi-cli",font=F(SANS,17),fill=mix(MUTED,PINK,0.4)+(255,),anchor="lm")

out_png = os.path.join(ROOT,"docs","social-preview.png")
final = base.convert("RGB").resize((W,H),Image.LANCZOS)
final.save(out_png)
sz = os.path.getsize(out_png)
if sz > 1_000_000:
    out_jpg = os.path.join(ROOT,"docs","social-preview.jpg")
    final.save(out_jpg, quality=88, optimize=True)
    print("PNG too big (%d KB) -> also wrote %s (%d KB)" % (sz//1024, out_jpg, os.path.getsize(out_jpg)//1024))
print("wrote", out_png, sz//1024, "KB")
