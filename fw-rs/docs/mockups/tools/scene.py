import sys; import sim
BG,BAR,RED=0x0883,0x1904,0xF145
TAG_X=[8,232]; VX=[38,260]; CX=354
def full(l,acdc,mode_g,unit,main,aux,old=False,prev_acdc=None):
    # mkey-change block
    l.label('UNIT',(401,114,79,32),unit,RED,BG,4,-1)
    if old:
        l.label('MODE',(437,76,32,32),mode_g,RED,BG,7)
        for x,g in [(8,"\x05"),(234,"\x04")]:
            if acdc: l.label('AUX_TAG',(x,41,24,21),g,RED,BG,8)
            else: l.fill(x,41,24,21,BG)
    else:
        l.label('MODE',(436,76,35,32),mode_g,RED,BG,7)
        if not acdc:
            for i in range(2): l.fill(TAG_X[i],36,VX[i]-TAG_X[i],30,BG)
    l.label('F78',(40,68,360,80),main,RED,BG,4)
    for i,a in enumerate(aux):
        vx,ux=(VX[i]+30,178 if i==0 else 400)
        if a is None:
            continue
        txt,u=a
        if acdc and not old:
            end=vx-30+138
            te=max(end-l.width('LETTERS',txt)-4,TAG_X[i]+24)
            l.label('AUX_TAG',(TAG_X[i],36,te-TAG_X[i],30),"\x05\x04"[i],RED,BG,4)
            l.label('LETTERS',(te,36,end-te,30),txt,RED,BG,4)
        else:
            l.label('LETTERS',(vx-30,36,138,30),txt,RED,BG,4)
        uw=54 if i==0 and acdc else 80
        l.label('LETTERS',(ux,36,uw,30),u,RED,BG,1)
def clear_aux(l,i):
    vx,ux=(VX[i]+30,178 if i==0 else 400); l.fill(vx-30,36,ux+80-vx+30,30,BG)
def compact(l,acdc,mode_g,unit,main,aux,old=False):
    l.label('LETTERS',(40,24,152,30),main,RED,BG,4)
    l.label('UNIT',(198,24,64,31),unit,RED,BG,1)
    if old: l.label('MODE',(266,23,32,32),mode_g,RED,BG,1); return
    l.label('MODE',(266,23,36,32),mode_g,RED,BG,1)
    if not acdc: l.fill(CX,22,480-CX,33,BG)
    if acdc:
        for i,a in enumerate(aux):
            y=23+16*i
            l.label('AUX_TAG',(CX,y,26,16),"\x05\x04"[i],RED,BG,1)
            l.label('F14',(CX+26,y,480-CX-26,16),a[0]+a[1],RED,BG,1,1)
