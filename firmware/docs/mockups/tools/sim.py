# Host-side mimic of fw-rs stockfont.rs label/glyphs/rbox for render checks.
import re
from PIL import Image
import os; SRC=os.path.join(os.path.dirname(os.path.abspath(__file__)),'../../../src/stockfont_data.rs')
src=open(SRC).read()
ADDR={}
for a,n in re.findall(r'0x([0-9A-F]+) => Some\(&(\w+)_FD\)',src): ADDR[n]=int(a,16)
def _blob(name):
    m=re.search(r'pub static %s_B: &\[u8\] = b"(.*?)"\s*;'%name,src,re.S)
    return bytes(m.group(1).replace('\\\n',''),'latin1').decode('unicode_escape').encode('latin1')
class Font:
    def __init__(s,name):
        m=re.search(r'pub static %s_FD: FontData = FontData\{line_h:(\d+),descent:(-?\d+),four:(\w+)'%name,src)
        s.lh,s.desc,s.four=int(m.group(1)),int(m.group(2)),m.group(3)=='true'
        m=re.search(r'pub static %s_E: \[Entry; \d+\] = \[(.*?)\];'%name,src,re.S)
        s.g={}
        for e in re.findall(r'Entry\{(.*?)\}',m.group(1)):
            d=dict((k,int(v,0)) for k,v in re.findall(r'(\w+):(-?\w+)',e)); s.g[d['code']]=d
        s.b=_blob(name)
    def alpha(s,g,x,y):
        p=x*g['h']+y
        if s.four:
            v=s.b[g['off']+p//2]; return (v>>4) if p%2==0 else v&15
        return 15 if s.b[g['off']+p//8]&(0x80>>(p&7)) else 0
FONTS={n:Font(n) for n in ['F78','UNIT','LETTERS','F21','F14','AUX_TAG','MODE','BTN_GLYPH']}
W,H=480,320
def rgb(c): return ((c>>11&31)*255//31,(c>>5&63)*255//63,(c&31)*255//31)
def blend(fg,bg,a):
    if a==0: return bg
    if a>=15: return fg
    ch=lambda c,s,m:(c>>s)&m
    mix=lambda s,m:((ch(bg,s,m)+(ch(fg,s,m)-ch(bg,s,m))*a//15)&m)<<s if True else 0
    # Rust: integer division truncates toward zero
    def mix(s,m):
        d=(ch(fg,s,m)-ch(bg,s,m))*a; q=abs(d)//15; q=q if d>=0 else -q
        return ((ch(bg,s,m)+q)&m)<<s
    return mix(11,31)|mix(5,63)|mix(0,31)
class LCD:
    def __init__(s,bg=0): s.px=[[bg]*W for _ in range(H)]
    def fill(s,x,y,w,h,c):
        for yy in range(max(0,y),min(H,y+h)):
            for xx in range(max(0,x),min(W,x+w)): s.px[yy][xx]=c
    def layout(s,f,r,text,align,sp):
        x,y,w,h=r; pen=0; xs=[]; gs=[]; lw=0
        for ch in text:
            g=f.g.get(ord(ch)); xs.append(pen); gs.append(g)
            if g: pen+=g['adv']+sp; lw=g['w']
            else: pen+=f.lh; lw=f.lh
        tw=(xs[-1]+lw) if xs else 0
        ox=0 if align<=2 else (w-tw if align<=5 else (w-tw)>>1)
        oy=0 if align in (0,3,6) else (h-f.lh if align in (2,5,7) else (h-f.lh)>>1)
        return [(g,x+ox+xs[i],y+oy+f.lh-f.desc-g['h']-g['yoff']) for i,g in enumerate(gs) if g]
    def label(s,fn,r,text,fg,bg,align,sp=0):
        f=FONTS[fn]; x,y,w,h=r
        pl=s.layout(f,r,text,align,sp)
        x0,y0=max(x,0),max(y,0); x1=min(x+w-1,W-1); y1=min(y+h-1,H-1)
        cover={}
        for gi,(g,px,py) in enumerate(pl):
            for gx in range(g['w']):
                if x0<=px+gx<=x1: cover[px+gx]=gi
        for yy in range(y0,y1+1):
            for xx in range(x0,x1+1):
                c=bg
                if xx in cover:
                    g,px,py=pl[cover[xx]]; gy=yy-py
                    if 0<=gy<g['h']: c=blend(fg,bg,f.alpha(g,xx-px,gy))
                s.px[yy][xx]=c
    def glyphs(s,fn,r,text,fg,bg,align,sp=0):
        f=FONTS[fn]
        for g,px,py in s.layout(f,r,text,align,sp):
            if px<0 or py<0: continue
            for gy in range(g['h']):
                for gx in range(g['w']):
                    if 0<=px+gx<W and 0<=py+gy<H: s.px[py+gy][px+gx]=blend(fg,bg,f.alpha(g,gx,gy))
    def rbox(s,r,rad,c,out):
        x,y,w,h=r
        for j in range(h):
            for i in range(w):
                dx=rad-i if i<rad else (i-(w-rad-1) if i>=w-rad else 0)
                dy=rad-j if j<rad else (j-(h-rad-1) if j>=h-rad else 0)
                if 0<=x+i<W and 0<=y+j<H: s.px[y+j][x+i]=c if dx*dx+dy*dy<=rad*rad else out
    def width(s,fn,text,sp=0):
        pl=s.layout(FONTS[fn],(0,0,0,0),text,0,sp)
        return (pl[-1][1]+pl[-1][0]['w']) if pl else 0
    def extent(s,x,y,w,h,bg):
        xs=[];ys=[]
        for yy in range(y,y+h):
            for xx in range(x,x+w):
                if s.px[yy][xx]!=bg: xs.append(xx); ys.append(yy)
        return (min(xs),max(xs),min(ys),max(ys)) if xs else None
    def save(s,path,crop=None,scale=2):
        im=Image.new('RGB',(W,H)); im.putdata([rgb(c) for row in s.px for c in row])
        if crop: im=im.crop(crop)
        im=im.resize((im.width*scale,im.height*scale),Image.NEAREST); im.save(path)
