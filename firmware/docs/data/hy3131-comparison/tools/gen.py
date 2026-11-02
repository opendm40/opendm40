"""Generate the per-mode comparison tables and the raw appendix from rows.json."""
import json, re, sys, os
sys.path.insert(0, os.path.dirname(__file__))
from hydec import decode

D = os.path.dirname(os.path.abspath(__file__))
rows = json.load(open(D + "/" + os.environ.get("ROWS", "rows.json")))
for e in rows:
    e["r"] = {int(k, 16): v for k, v in e["regs"].items()}
    e["d"] = decode(e["r"])

SRCS = ["hycon", "dm40", "121gw", "digilent", "temco", "owon", "owon-xdm1041", "keysight", "metrix"]
HEAD = {"hycon": "Hycon ENOB tool rows", "dm40": "DM40 stock", "121gw": "121GW",
        "digilent": "Digilent", "temco": "Temco", "owon": "OWON XDM2041", "owon-xdm1041": "OWON XDM1041", "keysight": "Keysight U128xA (mode mapping inferred)",
        "metrix": "Metrix MTX3293"}
APD = json.load(open(D + "/apd.json")) if os.path.exists(D + "/apd.json") else {}


def short(e):
    s, l = e["src"], e["label"]
    if s == "121gw":
        l = l.split(" / ")[0].split(",")[0]
        l = l.replace(" (600.0V)", "").replace(" [1 ms peak table]", "").replace("(1ms peak)", "1ms-pk")
        l = re.sub(r"^(DCV|ACV|DCmV|ACmV|DCA|ACA) ", "", l)
        return l.replace("Ω", " ohm").replace("ohm", "Ω").strip()
    if s == "digilent":
        return l.split(" (INTE")[0].replace(" DC", "").replace(" AC", "")
    if s == "temco":
        l = l.replace(" Manual", "").replace(" Auto", " auto")
        m = re.match(r"(OHMCV|OHMCC|CapCV|CapCC) row (\d+)", l)
        if m:
            return {"OHMCV": "CV", "OHMCC": "CC", "CapCV": "CV", "CapCC": "CC"}[m.group(1)] + m.group(2)
        l = re.sub(r"^(DCVolt|ACVolt|DCmVolt|ACmVolt|DCmA|DCA|ACmA|ACA|OHM|Cap|Freq|Diode|Continuity|Temp) ", "", l)
        return l
    if s == "owon-xdm1041":
        if l.startswith("TEMP"):
            return l.split(" (")[0].split(",")[0].replace("TEMP range ", "TEMP r")
        return l.split(" [")[0].replace("CONF:", "").replace("VOLT:", "").replace("CURR:", "")
    if s == "owon":
        if "TEMP" in l:
            return "TC (TEMP:RTD KITS90 capture)"
        m = re.search(r"FRES \(after RES (\S+)\)", l)
        if m:
            return "FRES " + m.group(1)
        l = l.split(" [")[0]
        return l.replace("CONF:", "").replace("VOLT:", "").replace("CURR:", "")
    if s == "keysight":
        m = re.match(r"mode (\d) r(\d)", l)
        if m:
            return f"m{m.group(1)} r{m.group(2)}"
        if l.startswith("row 1:"):
            return "row 1 (mode 0x0B)"
        return l.split(":")[0]
    if s == "hycon":
        return l.replace("/HYCONFIG-", "-")
    if s == "dm40":
        l = l.replace("mA/uA r0 (60 mA, 600 uA)", "mA,µA r0").replace("mA/uA r1 (600 mA, 6000 uA)", "mA,µA r1").replace("A 6 A", "6 A").replace("A 10 A", "10 A")
        return l.replace(" (never selected)", "*").replace("P8CLR ", "").replace(" (use unknown)", "").replace(" (thermocouple, used)", "")
    return l


def view(e):
    d, r = e["d"], e["r"]
    ad1 = d["ENAD1"] == "1"
    ad2 = d["ENAD2"] == "1"
    v = {}
    off1 = "AD1 off"
    if "AD1 fields left from the previous mode" in e["label"]:
        ad1, off1 = False, "previous-mode leftover"
    v["AD1 input (+ / −)"] = d["AD1 in"] if ad1 else off1
    v["AD1 reference (+ / −)"] = d["AD1 ref"] if ad1 else off1
    v["AD1IG"] = d["AD1IG"] if ad1 else off1
    v["AD1RG"] = d["AD1RG"] if ad1 else off1
    v["R22: OSR1, AD1CHOP, DCOS (period)"] = d["R22 AD1"] if ad1 else off1
    v["AD1 buffers (H L P N)"] = d["AD1 buffers"] if ad1 else off1
    v["SFT1 pre-filter R"] = d["SFT1"]
    v["OP1 (+ input, OPS1, chop)"] = d["OP1"] if r[0x32] & 8 else "off"
    v["OP2 (+ input, OPS2)"] = d["OP2"] if r[0x32] & 0x80 else "off"
    v["PA switches"] = d["PA switches"]
    v["R2F ENVS, SMODE"] = d["R2F SMODE"]
    v["OP3 VREF (SFUVR)"] = d["OP3 VREF"]
    v["SDIO (PB0–PB8)"] = d["SDIO"]
    v["REFO, AGND, ACM"] = d["refs"].replace(" ENBIAS=1", "")
    v["ACC (R30)"] = d["ACC"]
    v["AD2 in / ref / gain"] = (f"{d['AD2 in']}; ref {d['AD2 ref']}; {d['AD2 gain']}") if ad2 else "off"
    v["R26 AD2: OSR2, clock, chop"] = d["R26 AD2"] if ad2 else "off"
    lr = r[0x29]
    if not (lr & 0xC4):
        v["R29 LPF / RMS / peak"] = "off"
    elif not ad2 and not ((lr & 4) and (lr & 3) == 1):
        v["R29 LPF / RMS / peak"] = f"inert, AD2 off (R29=0x{lr:02x})"
    else:
        v["R29 LPF / RMS / peak"] = d["R29 LPF/RMS/PKH"]
    if 0x34 in r:
        a3 = r[0x34]
        if a3 & 0x80:
            v["AD3, SVXI, SDO23 (R34, R35)"] = d["AD3"] + "; in " + d["AD3 in"]
        else:
            v["AD3, SVXI, SDO23 (R34, R35)"] = f"AD3 off (R34=0x{a3:02x})" if a3 else "off (0x00)"
    else:
        v["AD3, SVXI, SDO23 (R34, R35)"] = "not written"
    v["SPHACAL (R36)"] = d["SPHACAL"]
    if r[0x20] & 0x10:
        v["Comparator (R20, R21)"] = d["R20 comparator"].replace("ENCMP=1 ", "on, ") + "; " + d["R21 thresholds"]
    else:
        v["Comparator (R20, R21)"] = "off"
    v["Counter, CNT, CMP pin"] = d["counter"]
    v["Clock"] = d["clock"]
    return v


FIELDS = list(view(rows[0]).keys())


def cell(es, f):
    if not es:
        return "—"
    groups = {}
    for e in es:
        groups.setdefault(view(e)[f], []).append(short(e))
    if len(groups) == 1:
        return next(iter(groups))
    parts = []
    items = sorted(groups.items(), key=lambda kv: -len(kv[1]))
    if len(items[0][1]) > len(items[1][1]):
        parts.append(items[0][0])
        items = items[1:]
    for val, labs in items:
        # dedupe labels
        seen = []
        for x in labs:
            if x not in seen:
                seen.append(x)
        parts.append(f"{val} ({', '.join(seen)})")
    return "<br>".join(parts)


def esc(s):
    return s.replace("|", "\\|")


def table(mode, exclude=lambda e: False, drop_const=("Clock",)):
    def inmode(e):
        if e["mode"] == mode:
            return True
        # the 121GW runs temperature on its DC mV 50 mV row (table comment)
        if mode == "freq" and e["src"] == "keysight" and e["label"].startswith("row 1:"):
            return True
        return mode == "temp" and e["src"] == "121gw" and "Temperature" in e["label"]
    per = {s: [e for e in rows if e["src"] == s and inmode(e)
               and (not exclude(e) or (mode == "freq" and e["src"] == "keysight"))] for s in SRCS}
    out = ["| field | " + " | ".join(HEAD[s] for s in SRCS) + " |",
           "|---|" + "---|" * len(SRCS)]
    for f in FIELDS:
        cells = [cell(per[s], f) for s in SRCS]
        if f in drop_const and len(set(c for c in cells if c != "—")) <= 1:
            continue
        apd = APD.get(mode, {}).get(f)
        if apd:
            cells[0] = (cells[0] + "<br>" if cells[0] != "—" else "") + "APD " + apd
        out.append(f"| {f} | " + " | ".join(esc(c) for c in cells) + " |")
    return "\n".join(out)


def excluded(e):
    if keysight_base_excluded(e):
        return True
    if e["src"] == "temco" and " Auto" in e["label"]:
        return True
    if e["src"] == "dm40" and ("never selected" in e["label"] or e["label"].startswith("P8SET")):
        return True
    if e["src"] == "owon-xdm1041" and ("phase B" in e["label"] or "state+0x448" in e["label"]):
        return True
    if e["src"] == "metrix" and ("AC+DC" in e["label"] or e["label"].startswith("mode 9") or "all zero" in e["label"]):
        return True
    return False


def keysight_base_excluded(e):
    # the composed per-range rows replace base rows 1, 4, 7 in the comparison
    return e["src"] == "keysight" and re.match(r"row (1|4|7):", e["label"]) is not None


def appendix():
    out = []
    names = {"hycon": "Hycon ENOB tool (HY313x ENOB V1.4, Config/*/HYCONFIG-n.txt)",
             "dm40": "Alientek DM40 stock V2.6", "121gw": "EEVblog 121GW v2.05",
             "digilent": "Digilent DMM Shield (dmm.cpp)", "temco": "Temco T3 (HY3131_FunTable.h)",
             "owon": "OWON XDM2041 V1.7.2 (SPI capture, bulk writes)",
             "owon-xdm1041": "OWON XDM1041 V2.1.2 (set_mode emulated from the stock image, bulk writes)",
             "keysight": "Keysight U1281A/U1282A v1.05 (decoded table 0x18EA and overlays)",
             "metrix": "Metrix MTX3293 / ASYC IV, image v1.02 (table at flash 0xB4C64, R1E..R36)"}
    for s in SRCS:
        out.append(f"### {names[s]}\n")
        out.append("| mode | label | R20 … | origin |")
        out.append("|---|---|---|---|")
        for e in rows:
            if e["src"] != s:
                continue
            first = min(e["r"])
            hx = " ".join(f"{e['r'][k]:02X}" for k in sorted(e["r"]))
            lab = esc(e["label"])
            if first != 0x20:
                lab += f" (from R{first:02X})"
            out.append(f"| {e['mode']} | {lab} | `{hx}` | {esc(e['origin'])} |")
        out.append("")
    return "\n".join(out)


if __name__ == "__main__":
    tpl = open(sys.argv[1]).read()
    def rep(m):
        key = m.group(1)
        if key == "APPENDIX":
            return appendix()
        return table(key, exclude=excluded)
    open(sys.argv[2], "w").write(re.sub(r"\{\{TABLE:(\w+)\}\}|\{\{(APPENDIX)\}\}",
                                        lambda m: rep(type("M", (), {"group": lambda self, i: m.group(1) or m.group(2)})()), tpl))
