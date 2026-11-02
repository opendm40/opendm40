"""HY3131 R20..R36 row decoder (field names and option tables from
emu/hy3131/ref/registers.md, DS-HY3131-V10_EN chapters 11-14)."""

SAD1FP = ["SENSE", "FB", "RLU", "OP1O", "OP2O", "VDDA", "REFO", "VREF"] + [f"PB{i}" for i in range(8)]
SAD1FN = ["SENSE", "RLU", "VSSA", "AGND", "PB2", "PB3", "PB4", "PB5"]
SAD1I_P = ["filter", "FB", "TS1P", "TS1N"]
SAD1I_N = ["filter", "RLU", "TS2N", "TS2P"]
SAD1RH = ["FB", "REFO", "VREF", "PB6", "RLU", "VDDA", "AGND", "-"]
SAD1RL = ["RLU", "AGND", "PB3", "PB5", "FB", "VSSA", "VREF", "-"]
SAD2IP = ["OP1O", "OP2O", "PB4", "PB7"]
SAD2IN = ["RLU", "AGND", "PB3", "PB5"]
SAD2RH = ["FB", "REFO", "VREF", "PB6"]
SAD2RL = ["RLU", "AGND", "PB3", "PB5"]
SOP1P = ["SENSE", "FB", "RLU", "AGND", "PB0", "PB1", "PB2", "PB8"]
SOP2P = ["SENSE", "FB", "RLU", "AGND", "PB0", "PB1", "PB2", "PB3"]
SCMPI = ["SENSE", "FB", "OP1O", "PB0", "PB1", "RLD", "PB3", "PB4"]
SCMPRH = ["VDSC16", "VDSC13", "VDSC11", "VDSC10", "VDSC9", "VDSC8", "VDSC7", "PB7",
          "AGNDP6", "AGNDP5", "AGNDP4", "AGNDP3", "AGNDP2", "AGNDP1", "AGNDP0", "AGNDN1"]
SCMPRL = ["VDSC2", "VDSC5", "VDSC7", "VDSC8", "VDSC9", "VDSC10", "VDSC11", "PB6",
          "AGNDN6", "AGNDN5", "AGNDN4", "AGNDN3", "AGNDN2", "AGNDN1", "AGNDP0", "AGNDP1"]
SFUVR0 = ["VDS17", "VDS16", "VDS15", "AGNDP9", "AGNDP8", "AGNDP7", "AGNDP6", "AGNDP0",
          "VDS1", "VDS2", "VDS3", "AGNDN9", "AGNDN8", "AGNDN7", "AGNDN6", "PB7"]
SFUVR1 = [("VDS17", "VDS1"), ("VDS16", "VDS2"), ("VDS15", "VDS3"), ("AGNDP9", "AGNDN9"),
          ("AGNDP8", "AGNDN8"), ("AGNDP7", "AGNDN7"), ("AGNDP6", "AGNDN6"), ("AGNDP0", "PB7")]
OP1CHOP = ["off", "1 kHz", "2 kHz", "on (static)"]
SFT1 = ["100k", "10k", "0", "open"]
AD1IG = ["0.9", "1.8", "2.7", "3.6"]
AD2IG = ["0.5", "1.0", "1.5", "2.0"]
AD1OS = ["0", "+0.25", "+0.5", "+0.75", "0", "-0.25", "-0.5", "-0.75"]
AD1CHOP = ["00 (VX+VOS)", "01 (VX-VOS)", "10 (chopped)", "11 (chopped)"]
AD2OSR = [32, 64, 128, 256, 512, 1024, 1024, 1024]
PKHSEL = ["AD2", "AD1<23:5>", "LPF", "LPF"]
SAGND = ["0.5", "0.3", "0.1", "float"]
SACM = ["1.2 V", "0.9 V", "1.5 V", "1.125 V"]
# SMODE<3:0> (+CMPO) -> SGND SVDD SVSS SVSO1 SVSO2 SCP SCN, DS fig. 12.2 truth table
PBUS = {0: "none", 1: "SGND", 2: "SVDD", 3: "SVSS", 4: "SVSO1", 5: "SGND+SVSO1",
        6: "SVDD+SVSO2+SCP", 7: "SVSS+SVSO2+SCN"}
PBUS_CMPO = {8: ("none", "SGND"), 10: ("SVDD", "SVSS"), 12: ("SVSO1", "SGND+SVSO1"),
             14: ("SVDD+SVSO2+SCP", "SVSS+SVSO2+SCN")}
FSYS = 4.9152e6


def b(r, reg, hi, lo=None):
    lo = hi if lo is None else lo
    v = r[reg]
    return (v >> lo) & ((1 << (hi - lo + 1)) - 1)


def pa_switches(r):
    out = []
    for n in range(9, -1, -1):
        reg = 0x2A + n // 2
        if reg not in r:
            continue
        nib = (r[reg] >> (4 * (n & 1))) & 0xF
        s = "".join(c for c, bit in zip("PDFS", (8, 4, 2, 1)) if nib & bit)
        if s:
            out.append(f"{s}{n}")
    return " ".join(out) if out else "none"


def decode(r):
    """r: dict reg -> byte (0x20.. at least 0x33). Returns ordered dict of fields."""
    d = {}
    has = lambda reg: reg in r
    # comparator / counter
    d["R20 comparator"] = (f"ENCMP={b(r,0x20,4)} SCMPI={SCMPI[b(r,0x20,7,5)]}")
    d["R21 thresholds"] = f"H={SCMPRH[b(r,0x21,7,4)]} L={SCMPRL[b(r,0x21,3,0)]}"
    d["counter"] = f"ENCTR={b(r,0x20,1)} ENCNTI={b(r,0x20,3)} ENPCMPO={b(r,0x20,2)}"
    # AD1
    enad1 = b(r, 0x23, 7)
    d["ENAD1"] = str(enad1)
    sad1i = b(r, 0x33, 1, 0)
    fp, fn = SAD1FP[b(r, 0x24, 7, 4)], SAD1FN[b(r, 0x24, 2, 0)]
    if sad1i == 0:
        d["AD1 in"] = f"{fp} - {fn}"
    else:
        d["AD1 in"] = f"{SAD1I_P[sad1i]} - {SAD1I_N[sad1i]} (SAD1I={sad1i}; filter mux {fp} - {fn})"
    d["AD1 ref"] = f"{SAD1RH[b(r,0x28,6,4)]} - {SAD1RL[b(r,0x28,2,0)]}"
    d["AD1IG"] = AD1IG[b(r, 0x25, 5, 4)]
    d["AD1RG"] = "1/3" if b(r, 0x23, 4) else "1"
    osr = 256 << b(r, 0x22, 2, 0)
    chop = b(r, 0x22, 4, 3)
    per = osr * 30 / FSYS * (2 if chop >= 2 else 1) * 1e3
    d["R22 AD1"] = f"OSR {osr}, CHOP {chop:02b}, DCOS {AD1OS[b(r,0x22,7,5)]} ({per:.1f} ms)"
    bufs = "".join(c for c, bit in zip(("H", "L", "P", "N"), (3, 2, 1, 0)) if b(r, 0x23, bit))
    d["AD1 buffers"] = bufs or "none"
    d["SFT1"] = SFT1[b(r, 0x33, 3, 2)]
    # op amps
    d["OP1"] = (f"{'on' if b(r,0x32,3) else 'off'} +{SOP1P[b(r,0x32,2,0)]} "
                f"OPS1={b(r,0x25,0)} chop {OP1CHOP[b(r,0x33,7,6)]}")
    d["OP2"] = f"{'on' if b(r,0x32,7) else 'off'} +{SOP2P[b(r,0x32,6,4)]} OPS2={b(r,0x25,1)}"
    # switch network
    d["PA switches"] = pa_switches(r)
    sm = b(r, 0x2F, 6, 0)
    lo = sm & 0xF
    bus = PBUS.get(lo) if lo < 8 else ("CMPO 0/1: %s / %s" % PBUS_CMPO[lo & 0xE] if (lo & 0xE) in PBUS_CMPO else "?")
    extra = []
    if sm & 0x10:
        extra.append("SMODE4 (RLU)")
    if sm & 0x20:
        extra.append("SMODE5 (ACC array)")
    if sm & 0x40:
        extra.append("SMODE6")
    d["R2F SMODE"] = f"ENVS={b(r,0x2F,7)} SMODE=0x{sm:02x}: bus {bus}" + (", " + ", ".join(extra) if extra else "")
    sf = b(r, 0x31, 3, 0)
    if lo & 8:
        p = SFUVR1[sf & 7]
        d["OP3 VREF"] = f"SFUVR={sf}: {p[0]} / {p[1]} (CMPO 0/1)"
    else:
        d["OP3 VREF"] = f"SFUVR={sf}: {SFUVR0[sf]}"
    d["SDIO"] = str(b(r, 0x24, 3))
    d["refs"] = (f"ENREFO={b(r,0x31,7)} SREFO={b(r,0x30,7)} ({'PB6' if b(r,0x30,7) else 'bandgap'}) "
                 f"ENBIAS={b(r,0x31,6)} AGND={SAGND[b(r,0x31,5,4)]} VDDA ACM={SACM[b(r,0x25,3,2)]}")
    d["ACC"] = f"{b(r,0x30,6,0)} ({b(r,0x30,6,0)*0.2:.1f} pF)"
    d["clock"] = f"ENOSC={b(r,0x33,5)} ENXI={b(r,0x33,4)}"
    # AD2 / LPF / RMS / peak
    enad2 = b(r, 0x26, 7)
    d["ENAD2"] = str(enad2)
    d["AD2 in"] = f"{SAD2IP[b(r,0x27,7,6)]} - {SAD2IN[b(r,0x27,5,4)]}"
    d["AD2 ref"] = f"{SAD2RH[b(r,0x27,3,2)]} - {SAD2RL[b(r,0x27,1,0)]}"
    d["AD2 gain"] = f"IG {AD2IG[b(r,0x25,7,6)]}, RG {'1/3' if b(r,0x26,4) else '1'}"
    d["R26 AD2"] = (f"OSR2 {AD2OSR[b(r,0x26,2,0)]}, CLK SYSCLK/{4 if b(r,0x26,3) else 2}, "
                    f"CHOP {b(r,0x26,5)}")
    d["R29 LPF/RMS/PKH"] = (f"ENRMS={b(r,0x29,7)} ENLPF={b(r,0x29,6)} OSR4 {128 << b(r,0x29,5,3)} "
                            f"ENPKH={b(r,0x29,2)} PKHSEL={PKHSEL[b(r,0x29,1,0)]}")
    if has(0x34):
        d["AD3"] = (f"ENAD3={b(r,0x34,7)} CHOP={b(r,0x34,5)} RG={'1/3' if b(r,0x34,4) else '1'} "
                    f"SVXI={b(r,0x34,3)} SDO23={b(r,0x34,2)}")
    else:
        d["AD3"] = "not written"
    if has(0x35):
        d["AD3 in"] = f"{SAD2IP[b(r,0x35,7,6)]} - {SAD2IN[b(r,0x35,5,4)]}, IG {AD2IG[b(r,0x35,3,2)]}"
    else:
        d["AD3 in"] = "not written"
    d["SPHACAL"] = f"0x{r[0x36]:02x}" if has(0x36) else "not written"
    return d


FIELDS = ["AD1 in", "AD1 ref", "AD1IG", "AD1RG", "R22 AD1", "AD1 buffers", "ENAD1", "SFT1",
          "OP1", "OP2", "PA switches", "R2F SMODE", "OP3 VREF", "SDIO", "refs", "ACC",
          "ENAD2", "AD2 in", "AD2 ref", "AD2 gain", "R26 AD2", "R29 LPF/RMS/PKH", "AD3", "AD3 in",
          "SPHACAL", "R20 comparator", "R21 thresholds", "counter", "clock"]
