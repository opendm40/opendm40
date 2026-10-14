//
// HY3131 DMM analog front end (Hycon, DS-HY3131-V10_EN) on the DM40's bit-banged
// SPI (SCK PC10, MOSI PC12, MISO PC11, CS PD2; firmware/re/analysis/hy3131_v2_6.h,
// fw-rs/src/hy3131.rs). Research, open points and the per-row analog model:
// emu/hy3131/RESEARCH.md and emu/hy3131/README.md.
//
// Two classes:
//   Hy3131Core  the chip: register file R00..R3F with the datasheet semantics
//               (clear-on-read INTF, enable bits clearing data, counter preset,
//               reset command, reserved bits), the conversion timing (AD1 =
//               OSR1 * 30 / SYSCLK, x2 with the chopper; AD2 = OSR2 * 2 / SYSCLK;
//               LPF / RMS at the measured period), the frequency counter (gate on
//               input edges), the comparator / CMP pin, peak hold, the
//               temperature sensor, and the input model (V / COM waveform, A
//               current, R / C / diode on the ohm path, thermocouple). It has no
//               Renode dependency and is unit-tested on the host
//               (emu/hy3131/tests). Time is passed in by the caller.
//   HY3131      the Renode peripheral: the SPI frame on the GPIO pins, virtual
//               time, the calibration record of the flash image, the monitor API
//               (SetVin / SetAd1 kept, SetInput "spec" added).
// Frame: cmd = (reg << 1) | rd, MSB first; a read has one extra SCK pulse after
// the command, then MISO is sampled with SCK low before each pulse; registers
// auto-increment. With CS high, DO carries the IRQ (INTF & INTE).
//
using System;
using System.Collections.Generic;
using System.Globalization;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    /// What the input terminals carry. Voltages in V, currents in A, R in ohm, C in F.
    public class Hy3131Input
    {
        public double Vdc, Vac, Vhz = 50, Vphase, Vnoise; // V / COM node: DC + AC (RMS) at Vhz
        public string Vshape = "sine";                     // sine | square | triangle
        public double Vduty = 0.5;                         // square only
        public double Idc, Iac, Ihz = 50;                  // A terminal current
        public string Ishape = "sine";
        public double Iduty = 0.5;
        public double R = double.PositiveInfinity;         // ohm between V and COM (ohm / continuity)
        public double C;                                   // F (capacitance), 0 = open
        public double DiodeVf = double.PositiveInfinity;   // forward voltage (infinity = open)
        public double TcTemp = 25, ChipTemp = 25;          // thermocouple hot junction / die (C)
        public bool TcOpen;                                // thermocouple not connected
        public double TcMv = double.NaN;                   // explicit thermocouple mV (overrides TcTemp)
    }

    public class Hy3131Core
    {
        public const double SysClk = 4915200.0;
        public const double Vref = 1.25;    // PB<6> external reference, buffered by REFO (SREFO = 1)
        public const double Vdda = 3.6;     // VDSC taps are fractions of this
        public const double AcNodeTau = 0.3;        // AC coupling time constant of the AC V node (acdc doc 6a)
        public const double Op1OffLevel = -0.6;     // OP1O level while OP1 is off, relative to AGND (empirical: entry transient)
        public const double HpfTransientGain = 1.5; // RMS seen while the node DC slews, per volt of remaining step
        public const double LpfRateDivisor = 0.5;   // measured LPF period = datasheet OSR4 * this (RESEARCH.md 3)

        public Hy3131Core()
        {
            Reset(0);
        }

        // ------------------------------------------------------------ registers
        public void Reset(double now)
        {
            Array.Clear(regs, 0, regs.Length);
            intf = 0; ctbov = false; acpo = false;
            ad1Start = now; ad1Done = 0; ad1Code = 0; ad1Last = now;
            ad2Start = now; lpfStart = now; lpfDone = 0;
            lpfCode = 0; rmsVal = 0; ad2Code = 0;
            pkhMin = 0x3FFFF; pkhMax = -0x40000;
            ctEnabled = false; ctStart = double.NaN; ctStop = double.NaN;
            ctA = 0; ctB = 0; ctC = 0;
            rampV = 0; rampUp = true; rampLast = now;
            vc = Op1OffLevel; vcLast = now;
            role = Role.None; roleMode = -1; roleRange = 0;
            Rewrites = 0;
            rng = new Random(12345);
        }

        /// Register read with the datasheet side effects (INTF cleared, CTBOV cleared).
        public byte Read(int reg, double now)
        {
            reg &= 0x3F;
            switch(reg)
            {
            case 0x00: return (byte)ad1Code;
            case 0x01: return (byte)(ad1Code >> 8);
            case 0x02: return (byte)(ad1Code >> 16);
            case 0x03: return (byte)ad2Code;
            case 0x04: return (byte)(ad2Code >> 8);
            case 0x05: return (byte)(ad2Code >> 16);
            case 0x06: return (byte)lpfCode;
            case 0x07: return (byte)(lpfCode >> 8);
            case 0x08: return (byte)(lpfCode >> 16);
            case 0x09: case 0x0A: case 0x0B: case 0x0C: case 0x0D:
                return (byte)(rmsVal >> (8 * (reg - 0x09)));
            case 0x0E: return (byte)pkhMin;
            case 0x0F: return (byte)(pkhMin >> 8);
            case 0x10: return (byte)(pkhMin >> 16);
            case 0x11: return (byte)pkhMax;
            case 0x12: return (byte)(pkhMax >> 8);
            case 0x13: return (byte)(pkhMax >> 16);
            case 0x14:
            {
                var v = (byte)((CntPin ? 0x80 : 0) | (acpo ? 0x40 : 0) | (cmpHigh ? 0x20 : 0) | (cmpLow ? 0x10 : 0) | (ctbov ? 1 : 0));
                ctbov = false;
                return v;
            }
            case 0x15: return (byte)ctC;
            case 0x16: return (byte)(ctC >> 8);
            case 0x17: return (byte)(ctC >> 16);
            case 0x18: return (byte)ctB;
            case 0x19: return (byte)(ctB >> 8);
            case 0x1A: return (byte)(ctB >> 16);
            case 0x1B: return (byte)CtaNow(now);
            case 0x1C: return (byte)(CtaNow(now) >> 8);
            case 0x1D: return (byte)(CtaNow(now) >> 16);
            case 0x1E:
            {
                var v = intf;
                intf = 0;
                return v;
            }
            default:
                return regs[reg];
            }
        }

        public byte Peek(int reg) => reg == 0x1E ? intf : regs[reg & 0x3F];

        public void Write(int reg, byte v, double now)
        {
            reg &= 0x3F;
            if(reg == 0x37)
            {
                if(v == 0x60) Reset(now);
                return;
            }
            if(reg < 0x1B)
            {
                return; // data registers
            }
            if(reg <= 0x1D)
            {
                // CTA<23:8> writable while ENCTR = 0; CTA<7:0> is held at 0.
                if(!ctEnabled && reg != 0x1B)
                {
                    regs[reg] = v;
                }
                return;
            }
            if(reg == 0x1E)
            {
                intf &= v; // 0s clear, 1s cannot be set
                return;
            }
            if(reg == 0x1F) { regs[reg] = (byte)(v & 0x1F); return; }
            if(reg >= 0x38) { regs[reg] = v; return; }

            var old = regs[reg];
            var masked = (byte)(v & WriteMask[reg - 0x20]);
            regs[reg] = masked;
            switch(reg)
            {
            case 0x20:
                SetCounterEnable((masked & 0x02) != 0, now);
                break;
            case 0x22:
                if(old != masked) RestartAd1(now);
                break;
            case 0x23:
                if(((old ^ masked) & 0x80) != 0)
                {
                    if((masked & 0x80) == 0) ad1Code = 0;
                    RestartAd1(now);
                }
                break;
            case 0x26:
                if(((old ^ masked) & 0x80) != 0)
                {
                    ad2Start = now;
                    if((masked & 0x80) == 0) ad2Code = 0;
                    RestartLpf(now);
                }
                else if(old != masked) { ad2Start = now; RestartLpf(now); }
                break;
            case 0x29:
                if((masked & 0x40) == 0) lpfCode = 0;
                if((masked & 0x80) == 0) rmsVal = 0;
                if((masked & 0x04) == 0) { pkhMax = -0x40000; pkhMin = 0x3FFFF; }
                if(((old ^ masked) & 0xF8) != 0) RestartLpf(now);
                break;
            case 0x34:
                if(((old ^ masked) & 0x80) != 0) ad3Start = now;
                break;
            }
            if(reg >= 0x20 && reg <= 0x35)
            {
                Rewrites++;
                DeriveRole(now);
            }
        }

        // ------------------------------------------------------------ time
        /// Advance the chip to `now` (seconds): completes conversions, counter gates, ramps.
        public void Update(double now)
        {
            if(now < lastUpdate) now = lastUpdate; // monotonic
            UpdateAcNode(now);
            UpdateAd1(now);
            UpdateRamp(now);
            UpdateAd2(now);
            UpdateLpf(now);
            UpdateCounter(now);
            lastUpdate = now;
        }

        // ------------------------------------------------------------ decoded fields
        public bool EnAd1 => (regs[0x23] & 0x80) != 0;
        public bool EnAd2 => (regs[0x26] & 0x80) != 0;
        public bool EnAd3 => (regs[0x34] & 0x80) != 0;
        public bool EnLpf => (regs[0x29] & 0x40) != 0;
        public bool EnRms => (regs[0x29] & 0x80) != 0;
        public bool EnPkh => (regs[0x29] & 0x04) != 0;
        public bool EnCmp => (regs[0x20] & 0x10) != 0;
        public bool EnCntI => (regs[0x20] & 0x08) != 0;
        public bool EnPCmpO => (regs[0x20] & 0x04) != 0;
        public bool EnOp1 => (regs[0x32] & 0x08) != 0;
        public int Osr1 => 256 << (regs[0x22] & 7);
        public bool Ad1Chop => (regs[0x22] & 0x10) != 0;
        public double Ad1Ig => new[] { 0.9, 1.8, 2.7, 3.6 }[(regs[0x25] >> 4) & 3];
        public double Ad1Rg => (regs[0x23] & 0x10) != 0 ? 1.0 / 3 : 1.0;
        public double Ad2Ig => new[] { 0.5, 1.0, 1.5, 2.0 }[(regs[0x25] >> 6) & 3];
        public double Ad2Rg => (regs[0x26] & 0x10) != 0 ? 1.0 / 3 : 1.0;
        public double Dcos => new[] { 0, 0.25, 0.5, 0.75, 0, -0.25, -0.5, -0.75 }[(regs[0x22] >> 5) & 7];
        public int Osr2 => new[] { 32, 64, 128, 256, 512, 1024, 1024, 1024 }[regs[0x26] & 7];
        public int Osr4 => 128 << ((regs[0x29] >> 3) & 7);
        public int Sad1I => regs[0x33] & 3;
        public int Sfuvr => regs[0x31] & 0x0F;
        public bool Svxi => (regs[0x34] & 0x08) != 0;
        public bool Sdo23 => (regs[0x34] & 0x04) != 0;

        /// AD1 conversion period in seconds (datasheet 13.1 + the chopper halving).
        public double Ad1Period => Osr1 * 30.0 / SysClk * (Ad1Chop ? 2 : 1);
        public double Ad2Period => Osr2 * ((regs[0x26] & 0x08) != 0 ? 4.0 : 2.0) / SysClk * ((regs[0x26] & 0x20) != 0 ? 2 : 1);
        public double LpfPeriod => Ad2Period * Osr4 * LpfRateDivisor;

        /// Comparator thresholds (V relative to AGND) from R21.
        public double CmpHighThreshold => RefNode(ScmprhNames[regs[0x21] >> 4]);
        public double CmpLowThreshold => RefNode(ScmprlNames[regs[0x21] & 0x0F]);

        // ------------------------------------------------------------ observable state
        public Hy3131Input Input = new Hy3131Input();
        public bool CalConsistent = true;      // apply the meter's calibration deviations (README "Calibration")
        public double NoiseScale = 1.0;        // 0 = noise free
        public bool BeatModel = false;         // empirical AD2/AD3 enable-order beat (RESEARCH.md 8.2)
        public bool CntPin;                    // CNT pin level (ENCNTI source); set by the board model
        public int ForcedAd1 = int.MinValue;   // SetAd1: raw code override
        public int ForcedAd2 = int.MinValue;
        public ulong Rewrites;                 // configuration writes (R20..R35)
        public void ForceIntf(byte bits) { intf |= bits; }
        public int Intf => intf;
        public int Ad1Code => ad1Code;
        public long RmsValue => rmsVal;
        public int LpfCode => lpfCode;
        public bool Acpo => acpo;
        public bool CmpPin => EnPCmpO && acpo;  // CMP output pin (continuity beeper)
        public bool Irq => (intf & regs[0x1F]) != 0;
        public string RoleName => role.ToString() + (role == Role.None ? "" : " m" + roleMode + " r" + roleRange);
        public int Mode => roleMode;
        public int Range => roleRange;
        public double RampNodeV => rampV;
        public double AcNodeVc => vc;
        public double LastAd1Time => ad1Last;

        /// Calibration record (stock layout, hy3131_v2_6.h): gain / offset per path.
        public float[] Cal = DefaultCal();

        public static float[] DefaultCal()
        {
            var c = new float[0x1B8 / 4];
            // gains 1, offsets 0 at the stock offsets
            foreach(var o in new[] { 0x04, 0x24, 0x34, 0x54, 0x64, 0x84, 0x94, 0xc4, 0xf4, 0x124, 0x15c, 0x164, 0x170 })
            {
                var n = GainCount(o);
                for(var i = 0; i < n; i++) c[o / 4 + i] = 1f;
            }
            return c;
        }

        private static int GainCount(int o)
        {
            switch(o)
            {
            case 0x04: case 0x34: case 0x64: return 4;
            case 0x24: case 0x54: case 0x84: return 2;
            case 0x94: case 0xc4: case 0xf4: return 6;
            case 0x124: case 0x170: return 8;
            default: return 1;
            }
        }

        private float CalGain(int off) => CalConsistent ? Cal[off / 4] : 1f;
        private float CalOffs(int off) => CalConsistent ? Cal[off / 4] : 0f;

        // ------------------------------------------------------------ input spec
        /// "dcv:1.234", "acv:1.0@50[,dc=0.5][,shape=square][,duty=0.3][,noise=1e-4]",
        /// "dca:0.5", "aca:0.1@60[,dc=..]", "ohm:1000", "short", "open", "diode:0.65",
        /// "cap:1e-6", "hz:1000[,duty=0.25][,amp=1.0]", "temp:100[,cj=25]", "tcmv:4.1",
        /// "chip:25", "tcopen". Several specs may be separated by ';'.
        public void SetInput(string spec)
        {
            foreach(var part in spec.Split(';'))
            {
                var s = part.Trim();
                if(s.Length == 0) continue;
                var kv = s.Split(new[] { ':' }, 2);
                var key = kv[0].Trim().ToLowerInvariant();
                var args = kv.Length > 1 ? kv[1].Split(',') : new string[0];
                var opts = new Dictionary<string, string>();
                string main = null;
                foreach(var a in args)
                {
                    var t = a.Trim();
                    if(t.Length == 0) continue;
                    var eq = t.IndexOf('=');
                    if(eq > 0) opts[t.Substring(0, eq).Trim().ToLowerInvariant()] = t.Substring(eq + 1).Trim();
                    else if(main == null) main = t;
                }
                double v = 0, hz = double.NaN;
                if(main != null)
                {
                    var at = main.IndexOf('@');
                    if(at >= 0)
                    {
                        hz = D(main.Substring(at + 1));
                        main = main.Substring(0, at);
                    }
                    v = D(main);
                }
                var inp = Input;
                switch(key)
                {
                case "dcv":
                    inp.Vdc = v; inp.Vac = 0; inp.Vnoise = Opt(opts, "noise", 0);
                    break;
                case "acv":
                    inp.Vac = v; inp.Vdc = Opt(opts, "dc", 0);
                    if(!double.IsNaN(hz)) inp.Vhz = hz;
                    inp.Vshape = opts.ContainsKey("shape") ? opts["shape"] : "sine";
                    inp.Vduty = Opt(opts, "duty", 0.5);
                    inp.Vnoise = Opt(opts, "noise", 0);
                    break;
                case "hz":
                    inp.Vhz = v; inp.Vac = Opt(opts, "amp", 1.0); inp.Vdc = Opt(opts, "dc", 0);
                    inp.Vshape = opts.ContainsKey("shape") ? opts["shape"] : "square";
                    inp.Vduty = Opt(opts, "duty", 0.5);
                    break;
                case "dca":
                    inp.Idc = v; inp.Iac = 0;
                    break;
                case "aca":
                    inp.Iac = v; inp.Idc = Opt(opts, "dc", 0);
                    if(!double.IsNaN(hz)) inp.Ihz = hz;
                    inp.Ishape = opts.ContainsKey("shape") ? opts["shape"] : "sine";
                    inp.Iduty = Opt(opts, "duty", 0.5);
                    break;
                case "ohm": case "r":
                    inp.R = v; inp.C = 0; inp.DiodeVf = double.PositiveInfinity;
                    break;
                case "short":
                    inp.R = 0; inp.DiodeVf = 0; inp.Vdc = 0; inp.Vac = 0; inp.C = 0;
                    break;
                case "open":
                    inp.R = double.PositiveInfinity; inp.DiodeVf = double.PositiveInfinity; inp.C = 0;
                    inp.Vdc = 0; inp.Vac = 0; inp.Idc = 0; inp.Iac = 0; inp.TcOpen = true;
                    break;
                case "diode":
                    inp.DiodeVf = v; inp.R = double.PositiveInfinity;
                    break;
                case "cap": case "c":
                    inp.C = v; inp.R = double.PositiveInfinity;
                    break;
                case "temp": case "tc":
                    inp.TcTemp = v; inp.TcOpen = false; inp.TcMv = double.NaN;
                    if(opts.ContainsKey("cj")) { inp.ChipTemp = D(opts["cj"]); }
                    break;
                case "tcmv":
                    inp.TcMv = v; inp.TcOpen = false;
                    break;
                case "tcopen":
                    inp.TcOpen = true;
                    break;
                case "chip": case "cj":
                    inp.ChipTemp = v;
                    break;
                case "noise":
                    NoiseScale = v;
                    break;
                case "cal":
                    CalConsistent = v != 0;
                    break;
                case "beat":
                    BeatModel = v != 0;
                    break;
                default:
                    throw new ArgumentException("unknown input key '" + key + "'");
                }
            }
        }

        private static double D(string s) => double.Parse(s.Trim(), NumberStyles.Float, CultureInfo.InvariantCulture);
        private static double Opt(Dictionary<string, string> o, string k, double dflt) => o.ContainsKey(k) ? D(o[k]) : dflt;

        // ============================================================ the chip
        private void RestartAd1(double now)
        {
            ad1Start = now; ad1Done = 0;
        }

        private void RestartLpf(double now)
        {
            lpfStart = now; lpfDone = 0;
        }

        private void UpdateAd1(double now)
        {
            if(!EnAd1) return;
            var p = Ad1Period;
            var n = (long)Math.Floor((now - ad1Start) / p);
            if(n <= ad1Done) return;
            ad1Done = n;
            ad1Last = ad1Start + n * p;
            UpdateRamp(ad1Last); // the ramp node as it was when the conversion ended
            ad1Code = Clamp24(ForcedAd1 != int.MinValue ? ForcedAd1 : ComputeAd1(ad1Last));
            intf |= 0x04;
            if(EnPkh && (regs[0x29] & 3) == 1) PeakHold(ad1Code >> 5);
        }

        private void UpdateAd2(double now)
        {
            if(!EnAd2) return;
            if(now - ad2Start < Ad2Period) return;
            intf |= 0x02;
            var x = ForcedAd2 != int.MinValue ? ForcedAd2 : Ad2Sample(now);
            ad2Code = Clamp19(Sdo23 ? x : x);
            if(EnPkh && (regs[0x29] & 3) == 0) PeakHold(ad2Code);
        }

        private void UpdateLpf(double now)
        {
            if(!EnAd2 || !(EnLpf || EnRms)) return;
            var p = LpfPeriod;
            var n = (long)Math.Floor((now - lpfStart) / p);
            if(n <= lpfDone) return;
            lpfDone = n;
            var t = lpfStart + n * p;
            double dc, acRms;
            Ad2Window(t, p, out dc, out acRms);
            if(EnLpf)
            {
                lpfCode = Clamp19((int)Math.Round(dc));
                intf |= 0x08;
                if(EnPkh && (regs[0x29] & 3) >= 2) PeakHold(lpfCode);
            }
            if(EnRms)
            {
                var sq = acRms * acRms;
                if(Svxi && !Sdo23 && EnAd3)
                {
                    // sum(X*Y)/N with X and Y the same node: equal to sum(X^2)/N unless the
                    // AD2/AD3 decimation phases differ (BeatModel, empirical).
                    if(BeatModel && beatBad) sq *= 1 + 0.11 * Math.Sin(2 * Math.PI * 2.04 * t);
                }
                // AD2 clips at +-262143 codes, so the mean square cannot exceed 262143^2
                // (the stock drops sqrt(RMS) > 0x40000 as garbage: a clipped input must
                // still produce an accepted, over-range reading)
                rmsVal = (long)Math.Min(sq, 262143.0 * 262143.0);
                intf |= 0x10;
            }
        }

        private void PeakHold(int v)
        {
            if(v > pkhMax) pkhMax = v;
            if(v < pkhMin) pkhMin = v;
        }

        private static int Clamp24(long v) => (int)Math.Max(-8388608, Math.Min(8388607, v));
        private static int Clamp19(int v) => Math.Max(-262144, Math.Min(262143, v));

        // ------------------------------------------------------------ counter
        private void SetCounterEnable(bool on, double now)
        {
            if(on == ctEnabled) return;
            ctEnabled = on;
            if(!on)
            {
                ctB = 0; ctC = 0; ctbov = false;
                regs[0x1B] = 0;
                ctStart = double.NaN; ctStop = double.NaN;
                ctEnableTime = now;
            }
            else
            {
                ctInit = (regs[0x1D] << 16) | (regs[0x1C] << 8);
                ctEnableTime = now;
                ctStart = double.NaN; ctStop = double.NaN;
                ctStartDecided = false;
            }
        }

        /// Live CTA value.
        private int CtaNow(double now)
        {
            if(!ctEnabled) return (regs[0x1D] << 16) | (regs[0x1C] << 8);
            if(!double.IsNaN(ctStop)) return ctA;
            var t0 = double.IsNaN(ctStart) ? ctEnableTime : ctStart;
            var ticks = (long)((now - t0) * SysClk);
            return (int)((ctInit + ticks) & 0xFFFFFF);
        }

        private void UpdateCounter(double now)
        {
            if(!ctEnabled || !double.IsNaN(ctStop)) return;
            double f, duty;
            CounterSignal(out f, out duty);
            if(f <= 0) return; // no edges: CTA runs, nothing else happens
            var period = 1.0 / f;
            if(!ctStartDecided)
            {
                // first rising edge after enable opens the gate
                var k = Math.Ceiling((ctEnableTime - ctPhase) / period);
                ctStart = ctPhase + k * period;
                ctStartDecided = true;
            }
            if(now < ctStart) return;
            var gate = (0x1000000 - ctInit) / SysClk;
            var overflowAt = ctStart + gate;
            var cycles = Math.Ceiling((overflowAt - ctStart) / period - 1e-9);
            var stop = ctStart + cycles * period;
            if(now < stop) return;
            ctStop = stop;
            var total = (long)Math.Round((stop - ctStart) * SysClk);
            ctA = (int)((ctInit + total) & 0xFFFFFF);
            var b = (long)cycles;
            if(b > 0xFFFFFF) { ctbov = true; b &= 0xFFFFFF; }
            ctB = (int)b;
            ctC = (int)Math.Min(0xFFFFFF, Math.Round(total * duty));
            intf |= 0x01;
        }

        /// The signal at the counter input: frequency (Hz) and high fraction.
        private void CounterSignal(out double f, out double duty)
        {
            f = 0; duty = 0;
            if(role == Role.CapOsc)
            {
                var r = CapOscR();
                var c = Input.C + CalOffs(0x190) * 1e-6;
                if(c <= 0) return;
                f = CalGain(0x170 + 4 * roleRange) * 0.72135 / (r * c);
                duty = 0.5;
                return;
            }
            if(EnCntI)
            {
                // CNT pin: the DM40 wiring is unknown; assume the same input (RESEARCH.md 8.4)
                SignalEdges(Input.Vac * NodeRatioAc(), out f, out duty, 0.0, 0.0);
                return;
            }
            if(!EnCmp) return;
            if(role == Role.AcV || role == Role.AcMv || role == Role.Freq || role == Role.DcV || role == Role.DcMv)
            {
                var ratio = NodeRatioAc();
                SignalEdges(Input.Vac * ratio, out f, out duty, CmpHighThreshold, CmpLowThreshold);
            }
            else if(role == Role.AcA || role == Role.DcA)
            {
                SignalEdges(Input.Iac * NodeRatioAc(), out f, out duty, CmpHighThreshold, CmpLowThreshold);
            }
        }

        private void SignalEdges(double rms, out double f, out double duty, double vrh, double vrl)
        {
            f = 0; duty = 0;
            var shape = Input.Vshape;
            var pk = Peak(rms, shape);
            if(pk <= Math.Max(Math.Abs(vrh), Math.Abs(vrl))) return;
            f = Input.Vhz;
            switch(shape)
            {
            case "square":
                duty = Input.Vduty;
                break;
            case "triangle":
                duty = 0.5 + (Math.Abs(vrl) - vrh) / (4 * pk);
                break;
            default:
                duty = 0.5 + (Math.Asin(Math.Min(1, Math.Abs(vrl) / pk)) - Math.Asin(Math.Min(1, vrh / pk))) / (2 * Math.PI);
                break;
            }
        }

        private static double Peak(double rms, string shape)
        {
            switch(shape)
            {
            case "square": return rms;
            case "triangle": return rms * Math.Sqrt(3);
            default: return rms * Math.Sqrt(2);
            }
        }

        // ------------------------------------------------------------ AC node (AD2 input)
        private void UpdateAcNode(double now)
        {
            var dt = now - vcLast;
            vcLast = now;
            if(dt <= 0) return;
            // the coupling capacitor charges to the OP1O DC level (or its off level)
            var src = EnOp1 && (role == Role.AcV || role == Role.DcV) ? Input.Vdc * NodeRatioAc() : Op1OffLevel;
            vc += (src - vc) * (1 - Math.Exp(-dt / AcNodeTau));
        }

        /// Instantaneous AD2 code.
        private int Ad2Sample(double t)
        {
            double dc, ac;
            NodeDcAc(t, out dc, out ac);
            var w = 2 * Math.PI * Input.Vhz * t;
            var pk = Peak(ac, Input.Vshape);
            double s;
            switch(Input.Vshape)
            {
            case "square": s = ((w / (2 * Math.PI)) % 1.0) < Input.Vduty ? pk : -pk; break;
            case "triangle": { var ph = (w / (2 * Math.PI)) % 1.0; s = pk * (ph < 0.5 ? 4 * ph - 1 : 3 - 4 * ph); break; }
            default: s = pk * Math.Sin(w); break;
            }
            return (int)Math.Round((dc + s) * Ad2Scale());
        }

        /// Mean and AC-RMS of the AD2 node over the LPF window, in AD2 codes.
        private void Ad2Window(double t, double window, out double dcCode, out double acCode)
        {
            double dc, ac;
            NodeDcAc(t, out dc, out ac);
            var scale = Ad2Scale();
            dcCode = dc * scale;
            var floor = 30.0 * NoiseScale;
            acCode = Math.Sqrt(ac * ac * scale * scale + floor * floor);
            if(ac > 0 && NoiseScale > 0) acCode += Gauss() * 1.0 * NoiseScale;
        }

        /// Codes per volt at the AD2 input: IG / (RG * VR) * 2^18.
        private double Ad2Scale() => Ad2Ig / (Ad2Rg * Vref) * 262144.0;

        /// DC level and AC RMS (V, at the AD2 input node) for the current role, with the
        /// calibration deviations folded in.
        private void NodeDcAc(double t, out double dc, out double ac)
        {
            dc = 0; ac = 0;
            switch(role)
            {
            case Role.AcV:
            {
                var r = roleRange;
                // display = sqrt(RMS) * VR / 2^18 * k and sqrt(RMS) = IG * Vnode / VR * 2^18
                // -> Vnode = display / (k * IG)
                ac = AcQ(Input.Vac, 0x34 + 4 * r, 0x44 + 4 * r) / (AcvK[r] * Ad2IgRow());
                var dcNode = DcQ(Input.Vdc, 0x64 + 4 * r, 0x74 + 4 * r) / (AcvK[r] * Ad2IgRow());
                var step = dcNode - vc;                                   // AC coupled: only the un-charged part is seen
                dc = step;
                ac = Math.Sqrt(ac * ac + Math.Pow(HpfTransientGain * step, 2));
                break;
            }
            case Role.AcMv:
            {
                var r = roleRange;
                ac = AcQ(Input.Vac * 1000, 0x54 + 4 * r, 0x5c + 4 * r) / 1000 * MvDiv[r] / Ad2IgRow();
                dc = DcQ(Input.Vdc * 1000, 0x84 + 4 * r, 0x8c + 4 * r) / 1000 * MvDiv[r] / Ad2IgRow();
                break;
            }
            case Role.AcA:
            {
                var m = roleMode; var r = roleRange; var i = (m - 4) * 2 + r;
                var unit = AUnit[m - 4];
                var g = Shunt[m - 4] * MvDiv[r];                         // V per unit of current (A / mA / uA)
                ac = AcQ(Input.Iac * unit, 0xc4 + 4 * i, 0xdc + 4 * i) * g / Ad2IgRow();
                dc = DcQ(Input.Idc * unit, 0xf4 + 4 * i, 0x10c + 4 * i) * g / Ad2IgRow();
                break;
            }
            case Role.Freq:
                ac = Input.Vac; dc = 0;
                break;
            case Role.DcV: case Role.DcMv:
                // AD2 off in these rows; the node still exists for the coupling model
                dc = Input.Vdc * NodeRatioAc(); ac = Input.Vac * NodeRatioAc();
                break;
            }
        }

        /// Quantity -> what the stock will display after calibration = input: Q' = (Q + offs) / gain.
        private double AcQ(double q, int gainOff, int offsOff) => (q + CalOffs(offsOff)) / CalGain(gainOff);
        private double DcQ(double q, int gainOff, int offsOff) => (q + CalOffs(offsOff)) / CalGain(gainOff);

        private double Ad2IgRow() => rowIg2;

        /// Node volts per input volt (or amp) for the AC-path roles.
        private double NodeRatioAc()
        {
            switch(role)
            {
            case Role.AcV: return 1.0 / (AcvK[roleRange] * rowIg2);
            case Role.DcV: return 1.0 / (AcvK[roleRange] * 1.0);
            case Role.AcMv: case Role.DcMv: return MvDiv[roleRange];
            case Role.AcA: return Shunt[roleMode - 4] * AUnit[roleMode - 4] * MvDiv[roleRange];
            case Role.DcA: return Shunt[roleMode - 7] * AUnit[roleMode - 7] * MvDiv[roleRange];
            default: return 1.0;
            }
        }

        // ------------------------------------------------------------ AD1
        /// AD1 code for a conversion ending at time t.
        private int ComputeAd1(double t)
        {
            double code;
            var ig = Ad1Ig; var rg = Ad1Rg;
            var fs = 8388608.0;
            switch(role)
            {
            case Role.DcV:
            {
                var r = roleRange;
                var q = Input.Vdc + Input.Vnoise * Gauss();
                // stock: V = AD1 * k * 1.38889 / 8388606 * gain - offs (+ negative-side gain, r0 correction)
                if(q < 0) q /= DcvNegK[r];
                if(r == 0 && Math.Abs(q) / 20000 > 3e-5) q -= 1e-4 - Math.Abs(q) / 20000;
                code = DcQ(q, 0x04 + 4 * r, 0x14 + 4 * r) / (AcvK[r] * 1.38889 / 8388606.0);
                code *= (ig / rowIg) / (rg / rowRg);
                break;
            }
            case Role.DcMv:
            {
                var r = roleRange;
                var q = Input.Vdc * 1000 + Input.Vnoise * 1000 * Gauss();
                code = DcQ(q, 0x24 + 4 * r, 0x2c + 4 * r) / (1.38889 / MvDiv[r] / 8388.606);
                code *= (ig / rowIg) / (rg / rowRg);
                break;
            }
            case Role.DcA:
            {
                var m = roleMode; var r = roleRange; var i = (m - 7) * 2 + r;
                var q = Input.Idc * AUnit[m - 7];
                code = DcQ(q, 0x94 + 4 * i, 0xac + 4 * i) / (1.25 / (Shunt[m - 7] * MvDiv[r] * 0.9) / 8388606.0);
                code *= (ig / rowIg) / (rg / rowRg);
                break;
            }
            case Role.Ohm:
            {
                var r = roleRange;
                var rk = Input.R / 1000.0;
                if(double.IsInfinity(rk)) { code = 1e12; break; }
                var q = (rk + CalOffs(0x144)) / CalGain(0x124 + 4 * r);
                code = q / (OhmRref[r] / 0.9 / 8388606.0);
                code *= ig / rowIg; // ratiometric: RG cancels
                break;
            }
            case Role.Cont:
            {
                var ro = Input.R;
                if(double.IsInfinity(ro)) { code = -1e12; break; }
                var q = (ro + CalOffs(0x160)) / CalGain(0x15c);
                code = -q / (1.0 / 0.9 / 8388.606);
                code *= ig / rowIg;
                break;
            }
            case Role.Diode:
            {
                var vf = Input.DiodeVf;
                if(double.IsInfinity(vf)) vf = 3.3; // source compliance: over range
                var q = (vf + CalOffs(0x168)) / CalGain(0x164);
                code = q / (1.25 * 10 / 0.9 / 8388606.0);
                code *= (ig / rowIg) / (rg / rowRg);
                break;
            }
            case Role.CapRamp: case Role.CapDischarge:
                // node volts -> code with the stock scale 1000 * 1.25 / 0.9 / 8388606 mV per code
                code = rampV / (1.25 / 0.9 / 8388606.0) * (ig / rowIg) / (rg / rowRg);
                break;
            case Role.Temp:
            {
                if(Sad1I >= 2)
                {
                    // internal sensor: codeA (SAD1I 11) - codeB (SAD1I 10) = (Tcj + 289.15 - cal) / 314.15 * 640000
                    var d = (Input.ChipTemp + 289.15 - CalOffs(0x194)) / 314.15 * 640000.0;
                    code = Sad1I == 3 ? d / 2 : -d / 2;
                    code *= ig / rowIg;
                }
                else
                {
                    double mv;
                    if(Input.TcOpen) mv = 200; // open thermocouple: far over range
                    else if(!double.IsNaN(Input.TcMv)) mv = Input.TcMv;
                    else mv = TypeK.Emf(Input.TcTemp) - TypeK.Emf(Input.ChipTemp);
                    code = DcQ(mv, 0x24, 0x2c) / (1.38889 * 100 / 8388606.0);
                    code *= (ig / rowIg) / (rg / rowRg);
                }
                break;
            }
            case Role.Freq:
                code = 0;
                break;
            default:
                code = 0;
                break;
            }
            if(role != Role.Temp || Sad1I < 2) code += Dcos / rg * fs;
            if(NoiseScale > 0 && role != Role.None)
            {
                code += Gauss() * 25.0 * Math.Sqrt(32768.0 / Osr1) * NoiseScale;
            }
            return (int)Math.Round(Math.Max(-1e9, Math.Min(1e9, code)));
        }

        private double Gauss()
        {
            var u1 = 1.0 - rng.NextDouble();
            var u2 = rng.NextDouble();
            return Math.Sqrt(-2 * Math.Log(u1)) * Math.Cos(2 * Math.PI * u2);
        }

        // ------------------------------------------------------------ capacitance
        private double CapOscR()
        {
            return CapK[roleRange] * 1000.0; // kohm -> ohm
        }

        private void UpdateRamp(double now)
        {
            var dt = now - rampLast;
            rampLast = now;
            if(dt <= 0) return;
            if(role == Role.CapDischarge)
            {
                rampV *= Math.Exp(-dt / 0.02);
                rampUp = true;
                return;
            }
            if(role != Role.CapRamp) return;
            var c = Input.C * 1e6; // uF
            if(c <= 0) c = 1e-4;   // open: ramps instantly to the threshold
            var slope = CalGain(0x170 + 4 * roleRange) * CapK[roleRange] / c; // V/s at the AD1 node
            var vh = CmpHighThreshold; var vl = CmpLowThreshold;
            if(vh < vl) { var x = vh; vh = vl; vl = x; }
            var remaining = dt;
            while(remaining > 0)
            {
                var target = rampUp ? vh : vl;
                var dist = Math.Abs(target - rampV);
                var tt = dist / slope;
                if(tt >= remaining)
                {
                    rampV += (rampUp ? 1 : -1) * slope * remaining;
                    break;
                }
                rampV = target;
                rampUp = !rampUp;
                acpo = !rampUp; // high after reaching the high threshold
                remaining -= tt;
            }
        }

        // ------------------------------------------------------------ comparator
        private bool cmpHigh, cmpLow;

        private void UpdateComparatorStatic()
        {
            if(!EnCmp) { acpo = false; cmpHigh = cmpLow = false; return; }
            switch(role)
            {
            case Role.Cont:
            {
                // OP1O against AGNDN<2> (-41.7 mV) with the 1 k reference at the SFUVR source:
                // beep (CMPO high) while the drop across Rx is inside the window.
                var isrc = OhmSourceVolts() / 1000.0;
                var thr = Math.Abs(CmpLowThreshold) / Math.Max(isrc, 1e-9);
                acpo = Input.R < thr;
                break;
            }
            case Role.Diode:
                acpo = true; // R21 = 0xFF: always high; the stock gates the pin with ENPCMPO
                break;
            case Role.CapRamp: case Role.CapOsc:
                break; // driven by the ramp / oscillator
            default:
                acpo = false;
                break;
            }
        }

        /// |VREF| of OP3 in volts for the ohm rows (SFUVR with SMODE<3> = 0).
        private double OhmSourceVolts()
        {
            var smode3 = (regs[0x2F] & 0x08) != 0;
            var name = smode3 ? SfuvrSmode3[Sfuvr, acpo ? 1 : 0] : SfuvrNames[Sfuvr];
            return Math.Abs(RefNode(name));
        }

        /// Voltage of a reference-divider node relative to AGND.
        public double RefNode(string name)
        {
            var agnd = Vdda * 0.5; // SAGND 00
            if(name.StartsWith("VDSC") || name.StartsWith("VDS"))
            {
                var n = int.Parse(name.Substring(name.StartsWith("VDSC") ? 4 : 3));
                return K1[n] * Vdda - agnd;
            }
            if(name.StartsWith("AGNDP")) return K2[int.Parse(name.Substring(5))] * Vref;
            if(name.StartsWith("AGNDN")) return -K2[int.Parse(name.Substring(5))] * Vref;
            if(name == "PB6") return Vref;
            if(name == "PB7") return 0;
            return 0;
        }

        // ------------------------------------------------------------ roles
        public enum Role { None, DcV, DcMv, AcV, AcMv, DcA, AcA, Ohm, Cont, Diode, CapOsc, CapRamp, CapDischarge, Freq, Temp }

        private void DeriveRole(double now)
        {
            var best = -1; var bestScore = int.MaxValue;
            for(var i = 0; i < Rows.Length; i++)
            {
                var row = Rows[i];
                var score = 0;
                for(var j = 0; j < 22; j++)
                {
                    var mask = RowMask[j];
                    // R32: SOP1P distinguishes the A / mA shunt rows (PB2 / PB1), so it is
                    // compared when the row uses OP1; ENOP1 itself is always masked (AC+DC
                    // writes R32 = 0x38 after the DC V rows, whose OP1 is off).
                    if(j == 18) mask = (row.Bytes[18] & 0x08) != 0 ? 0xF7 : 0xF0;
                    var d = (row.Bytes[j] ^ regs[0x20 + j]) & mask;
                    if(d != 0) score += BitCount(d) * (j == 0 ? 1 : 2);
                }
                if(score < bestScore) { bestScore = score; best = i; }
            }
            bestRow = best; bestScore_ = bestScore;
            var newRole = Role.None; var newMode = -1; var newRange = 0;
            if(best >= 0 && bestScore <= 2)
            {
                newRole = Rows[best].Role; newMode = Rows[best].Mode; newRange = Rows[best].Range;
                rowIg = new[] { 0.9, 1.8, 2.7, 3.6 }[(Rows[best].Bytes[5] >> 4) & 3];
                rowRg = (Rows[best].Bytes[3] & 0x10) != 0 ? 1.0 / 3 : 1.0;
                rowIg2 = new[] { 0.5, 1.0, 1.5, 2.0 }[(Rows[best].Bytes[5] >> 6) & 3];
            }
            if(newRole == Role.Temp && Sad1I == 1) newRole = Role.None;
            if(newRole != role || newMode != roleMode || newRange != roleRange)
            {
                if(newRole == Role.CapRamp && role != Role.CapRamp) { rampUp = true; }
                role = newRole; roleMode = newMode; roleRange = newRange;
            }
            // the AD2/AD3 enable-order phase: bad when ENAD2 was set before ENAD3 (empirical)
            beatBad = EnAd2 && EnAd3 && ad2Start <= ad3Start;
            UpdateComparatorStatic();
        }

        private static int BitCount(int v) { var c = 0; while(v != 0) { c += v & 1; v >>= 1; } return c; }

        private class RowDef
        {
            public Role Role; public int Mode, Range; public byte[] Bytes;
            public RowDef(Role role, int mode, int range, string hex)
            {
                Role = role; Mode = mode; Range = range;
                var parts = hex.Split(' ');
                Bytes = new byte[22];
                for(var i = 0; i < 22; i++) Bytes[i] = Convert.ToByte(parts[i], 16);
            }
        }

        // Compare masks per configuration byte R20..R35 (0 = ignored): R20 keeps SCMPI/ENCMP,
        // R21 / R22 / R36 ignored, R26 / R34 without the enable bits (fw-rs sets them after the
        // row), R31 without SFUVR (ohm variants), R32 without ENOP1 / SOP1P (AC+DC writes 0x38
        // after the DC V row), R33 without SAD1I (temperature phases).
        private static readonly int[] RowMask = {
            0xF0, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0x7F, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xF0, 0xF7, 0xFC, 0x7F, 0xFF };

        // Stock DM40 V2.6 configuration rows (firmware/re/analysis/hy3131_v2_6.h) with their role.
        private static readonly RowDef[] Rows = {
            new RowDef(Role.AcV, 0, 0, "f2 dd 16 03 33 43 80 9d 31 fc 20 00 00 09 28 a0 80 c7 38 ac 88 94"),
            new RowDef(Role.AcV, 0, 1, "f2 dd 16 03 33 40 80 9d 31 fc 20 00 00 09 28 a0 80 c7 38 ac 88 94"),
            new RowDef(Role.AcV, 0, 2, "f2 dd 16 03 33 40 80 9d 31 fc 20 00 90 00 28 a0 80 c7 38 ac 88 94"),
            new RowDef(Role.AcV, 0, 3, "f2 dd 16 03 33 40 80 9d 31 fc 20 00 99 00 28 a0 80 c7 38 ac 88 94"),
            new RowDef(Role.DcV, 1, 0, "f0 dd 15 83 31 40 00 55 31 00 06 00 00 90 28 a0 80 d7 38 ac 00 54"),
            new RowDef(Role.DcV, 1, 1, "f0 dd 15 83 07 40 00 55 31 00 06 00 00 09 68 a4 80 c7 33 ac 00 54"),
            new RowDef(Role.DcV, 1, 2, "f0 dd 15 83 07 40 00 55 31 00 06 00 90 00 68 a4 80 c7 33 ac 00 54"),
            new RowDef(Role.DcV, 1, 3, "f0 dd 15 83 07 40 00 55 31 00 06 00 99 00 68 a4 80 c7 33 ac 00 54"),
            new RowDef(Role.AcMv, 2, 0, "f2 dd 16 03 33 43 80 5d 31 f8 00 00 00 00 08 91 80 c7 aa ac 88 54"),
            new RowDef(Role.AcMv, 2, 1, "f2 dd 16 03 33 40 80 5d 31 f8 00 00 00 00 08 91 80 c7 aa ac 88 54"),
            new RowDef(Role.DcMv, 3, 0, "f0 dd 15 83 33 01 00 55 31 00 00 00 00 00 08 91 80 d7 3a ac 00 50"),
            new RowDef(Role.DcMv, 3, 1, "f0 dd 15 83 33 00 00 55 31 00 00 00 00 00 08 91 80 d7 3a ac 00 50"),
            new RowDef(Role.AcA, 4, 0, "f2 dd 15 03 52 43 80 2d 31 f4 00 00 00 00 08 81 80 c7 3e ac 88 24"),
            new RowDef(Role.AcA, 4, 1, "f2 dd 15 03 52 40 80 2d 31 f4 00 00 00 00 08 81 80 c7 3e ac 88 24"),
            new RowDef(Role.AcA, 5, 0, "f2 dd 14 03 35 43 80 2d 31 fc 00 00 00 00 08 81 80 c7 3d ac 88 24"),
            new RowDef(Role.AcA, 5, 1, "f2 dd 14 03 35 40 80 2d 31 fc 00 00 00 00 08 81 80 c7 3d ac 88 24"),
            new RowDef(Role.DcA, 7, 0, "f0 dd 15 80 35 01 00 55 31 00 00 00 00 00 08 81 80 c7 3e ac 00 50"),
            new RowDef(Role.DcA, 7, 1, "f0 dd 15 83 35 00 00 55 31 00 00 00 00 00 08 81 80 c7 3e ac 00 50"),
            new RowDef(Role.DcA, 8, 0, "f0 dd 15 83 35 01 00 55 31 00 00 00 00 00 08 81 80 c7 3d a4 00 50"),
            new RowDef(Role.DcA, 8, 1, "f0 dd 15 83 35 00 00 55 31 00 00 00 00 00 08 81 80 c7 3d ac 00 50"),
            new RowDef(Role.Ohm, 10, 0, "e0 dd 14 83 35 01 00 55 00 00 00 46 06 00 00 94 80 d2 3c 68 00 50"),
            new RowDef(Role.Ohm, 10, 1, "e0 dd 14 83 35 00 00 55 00 00 00 40 06 00 00 94 80 d2 3c 68 00 50"),
            new RowDef(Role.Ohm, 10, 2, "e0 dd 14 83 35 00 00 55 00 00 00 40 60 00 00 94 80 d2 3c 68 00 50"),
            new RowDef(Role.Ohm, 10, 3, "e0 dd 14 83 35 00 00 55 00 00 00 40 00 06 00 94 80 d2 3c 68 00 50"),
            new RowDef(Role.Ohm, 10, 4, "e0 dd 14 8f 35 08 00 55 00 00 00 06 00 60 00 94 80 d2 3c 68 00 50"),
            new RowDef(Role.Ohm, 10, 5, "e0 dd 14 8f 35 08 00 55 00 00 00 06 00 00 00 94 80 d2 3c 60 00 50"),
            new RowDef(Role.Ohm, 10, 6, "e0 dd 14 83 35 00 00 55 11 00 00 08 00 00 00 86 80 d1 3c 60 00 50"),
            new RowDef(Role.Cont, 11, 0, "56 ec 13 83 35 08 00 55 44 00 00 40 06 00 01 94 80 cd 38 68 00 50"),
            new RowDef(Role.Diode, 12, 0, "b0 ff 13 83 3d 00 00 55 31 00 80 00 88 00 00 86 80 e2 3c 68 00 50"),
            new RowDef(Role.CapOsc, 13, 0, "72 21 16 03 b5 00 00 55 31 00 00 08 00 80 80 9a 80 c7 3c ac 00 50"),
            new RowDef(Role.CapOsc, 13, 1, "72 21 16 03 b5 00 00 55 31 00 00 08 00 80 80 9a 80 c7 3c ac 00 50"),
            new RowDef(Role.CapOsc, 13, 2, "72 21 16 03 b5 00 00 55 11 00 00 00 00 08 00 9a 80 c7 3c ac 00 50"),
            new RowDef(Role.CapOsc, 13, 3, "72 21 16 03 b5 00 00 55 11 00 00 00 80 08 00 9a 80 c7 3c ac 00 50"),
            new RowDef(Role.CapOsc, 13, 4, "72 22 16 03 b5 00 00 55 11 00 80 00 80 00 00 9a 80 c7 3c ac 00 50"),
            new RowDef(Role.CapRamp, 13, 5, "52 bb 14 83 43 40 00 5d 31 00 00 00 00 88 00 8e 80 c2 cc a8 00 54"),
            new RowDef(Role.CapRamp, 13, 6, "52 bb 14 83 43 40 00 5d 31 00 00 00 80 08 00 8e 80 c2 cc a8 00 54"),
            new RowDef(Role.CapRamp, 13, 7, "52 bb 14 83 43 40 00 5d 31 00 00 00 88 08 00 8e 80 c2 cc a8 00 54"),
            new RowDef(Role.CapDischarge, 13, 8, "52 dd 13 83 43 40 00 5d 31 00 00 00 88 88 00 8e 80 c2 cc a8 00 54"),
            new RowDef(Role.Freq, 14, 0, "f2 dd 15 83 df 00 80 15 11 f8 00 00 00 00 08 91 80 c7 3a 2f 00 50"),
            new RowDef(Role.Temp, 15, 0, "f0 dd 16 83 33 01 00 55 31 00 00 06 00 60 08 95 80 d7 3c af 00 50"),
            new RowDef(Role.Temp, 15, 1, "f0 dd 16 83 33 00 00 55 31 00 00 00 00 06 08 95 80 d7 3c af 00 50"),
        };

        // Write masks of R20..R35 (datasheet "0" bits are held at 0, "-" bits are kept).
        private static readonly byte[] WriteMask = {
            0xFE, 0xFF, 0xFF, 0xDF, 0xFF, 0xFF, 0xBF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xBC, 0xFC,
            0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF };

        // Stock scale constants (dmm_v2_6.h)
        private static readonly double[] AcvK = { 10, 100, 1000, 10000 };
        private static readonly double[] MvDiv = { 10, 1 };
        private static readonly double[] Shunt = { 0.01, 0.0005, 4.75e-5 }; // ohm / kohm / Mohm
        private static readonly double[] AUnit = { 1, 1e3, 1e6 };           // A -> A / mA / uA
        private static readonly double[] OhmRref = { 0.1, 1, 10, 100, 1000, 10000, 63000 };
        private static readonly double[] CapK = { 1020, 1003, 102.7, 10.68, 1.5, 4.06, 41, 400 };
        private static readonly double[] DcvNegK = { 1.0, 1.00008, 1.00011, 1.0015 };
        private static readonly double[] K1 = { 0, 1 / 36.0, 2 / 36.0, 4 / 36.0, 8 / 36.0, 10 / 36.0, 12 / 36.0, 14 / 36.0, 16 / 36.0, 18 / 36.0,
            20 / 36.0, 22 / 36.0, 24 / 36.0, 26 / 36.0, 28 / 36.0, 32 / 36.0, 34 / 36.0, 35 / 36.0 };
        private static readonly double[] K2 = { 0, 2 / 120.0, 4 / 120.0, 6 / 120.0, 10 / 120.0, 20 / 120.0, 40 / 120.0, 60 / 120.0, 80 / 120.0, 100 / 120.0 };
        private static readonly string[] ScmprhNames = { "VDSC16", "VDSC13", "VDSC11", "VDSC10", "VDSC9", "VDSC8", "VDSC7", "PB7",
            "AGNDP6", "AGNDP5", "AGNDP4", "AGNDP3", "AGNDP2", "AGNDP1", "AGNDP0", "AGNDN1" };
        private static readonly string[] ScmprlNames = { "VDSC2", "VDSC5", "VDSC7", "VDSC8", "VDSC9", "VDSC10", "VDSC11", "PB6",
            "AGNDN6", "AGNDN5", "AGNDN4", "AGNDN3", "AGNDN2", "AGNDN1", "AGNDP0", "AGNDP1" };
        private static readonly string[] SfuvrNames = { "VDS17", "VDS16", "VDS15", "AGNDP9", "AGNDP8", "AGNDP7", "AGNDP6", "AGNDP0",
            "VDS1", "VDS2", "VDS3", "AGNDN9", "AGNDN8", "AGNDN7", "AGNDN6", "PB7" };
        private static readonly string[,] SfuvrSmode3 = {
            { "VDS17", "VDS1" }, { "VDS16", "VDS2" }, { "VDS15", "VDS3" }, { "AGNDP9", "AGNDN9" },
            { "AGNDP8", "AGNDN8" }, { "AGNDP7", "AGNDN7" }, { "AGNDP6", "AGNDN6" }, { "AGNDP0", "PB7" },
            { "VDS17", "VDS1" }, { "VDS16", "VDS2" }, { "VDS15", "VDS3" }, { "AGNDP9", "AGNDN9" },
            { "AGNDP8", "AGNDN8" }, { "AGNDP7", "AGNDN7" }, { "AGNDP6", "AGNDN6" }, { "AGNDP0", "PB7" } };

        private readonly byte[] regs = new byte[64];
        private byte intf;
        private bool ctbov, acpo, beatBad;
        private double ad1Start, ad1Last, ad2Start, ad3Start, lpfStart, lastUpdate;
        private long ad1Done, lpfDone;
        private int ad1Code, ad2Code, lpfCode, pkhMin, pkhMax;
        private long rmsVal;
        private bool ctEnabled, ctStartDecided;
        private double ctEnableTime, ctStart, ctStop, ctPhase;
        private int ctInit, ctA, ctB, ctC;
        private double rampV, rampLast;
        private bool rampUp;
        private double vc, vcLast;
        private Role role;
        private int roleMode, roleRange;
        private double rowIg = 0.9, rowRg = 1.0, rowIg2 = 1.0;
        private int bestRow = -1, bestScore_;
        /// Diagnostics: the closest stock row and the differing configuration bytes.
        public string RoleDebug
        {
            get
            {
                var r = string.Format("best {0} score {1}", bestRow, bestScore_);
                if(bestRow >= 0) for(var j = 0; j < 22; j++) { var d = (Rows[bestRow].Bytes[j] ^ regs[0x20 + j]) & RowMask[j]; if(d != 0) r += string.Format(" R{0:X2}:{1:X2}^{2:X2}", 0x20 + j, Rows[bestRow].Bytes[j], regs[0x20 + j]); }
                return r;
            }
        }
        private Random rng;
    }

    /// NIST ITS-90 type K thermocouple EMF (mV) from temperature (C).
    public static class TypeK
    {
        private static readonly double[] CNeg = { 0.0, 0.394501280250e-1, 0.236223735980e-4, -0.328589067840e-6, -0.499048287770e-8,
            -0.675090591730e-10, -0.574103274280e-12, -0.310888728940e-14, -0.104516093650e-16, -0.198892668780e-19, -0.163226974860e-22 };
        private static readonly double[] CPos = { -0.176004136860e-1, 0.389212049750e-1, 0.185587700320e-4, -0.994575928740e-7, 0.318409457190e-9,
            -0.560728448890e-12, 0.560750590590e-15, -0.320207200030e-18, 0.971511471520e-22, -0.121047212750e-25 };

        public static double Emf(double t)
        {
            double e = 0;
            if(t < 0)
            {
                for(var i = CNeg.Length - 1; i >= 0; i--) e = e * t + CNeg[i];
            }
            else
            {
                for(var i = CPos.Length - 1; i >= 0; i--) e = e * t + CPos[i];
                e += 0.118597600000 * Math.Exp(-0.118343200000e-3 * (t - 126.9686) * (t - 126.9686));
            }
            return e;
        }
    }

    // ================================================================= Renode peripheral
    public class HY3131 : IDoubleWordPeripheral, IKnownSize, IGPIOReceiver
    {
        public HY3131(IMachine machine, INumberedGPIOOutput portC, INumberedGPIOOutput portD,
                      int sck = 10, int miso = 11, int mosi = 12, int cs = 2)
        {
            this.machine = machine;
            this.portC = portC;
            this.misoPin = miso;
            Core = new Hy3131Core();
            Reset();
            // Connect() reports the current pin levels at once (CS is low before the
            // firmware drives it): the state is initialised before the handlers run.
            portC.Connections[sck].Connect(this, 0);
            portC.Connections[mosi].Connect(this, 1);
            portD.Connections[cs].Connect(this, 2);
        }

        public void Reset()
        {
            Core.Reset(Now);
            selected = false; sck = false; mosi = false;
            rising = 0; falling = 0; shift = 0; cmdDone = false; reg = 0; rd = false; wbyte = 0;
            Transfers = 0;
            calLoaded = false;
        }

        public long Size => 0x100;
        public uint ReadDoubleWord(long offset) => offset == 0 ? (uint)Core.Ad1Code : 0;
        public void WriteDoubleWord(long offset, uint value) { }

        public Hy3131Core Core { get; private set; }

        // ---- scripting (monitor) ----
        /// Raw AD1 code override (int.MinValue = off).
        public void SetAd1(int raw) { Core.ForcedAd1 = raw; }
        public void ClearAd1() { Core.ForcedAd1 = int.MinValue; }
        public void SetAd2(int raw) { Core.ForcedAd2 = raw; }
        /// DC voltage on the V / COM input (kept for the older scripts).
        public void SetVin(double volts) { Core.ForcedAd1 = int.MinValue; Core.Input.Vdc = volts; Core.Input.Vac = 0; }
        /// Input spec, see Hy3131Core.SetInput.
        public void SetInput(string spec) { Core.ForcedAd1 = int.MinValue; Core.SetInput(spec); this.Log(LogLevel.Info, "input: {0}", spec); }
        public void SetOption(string name, double value) { Core.SetInput(name + ":" + value.ToString(CultureInfo.InvariantCulture)); }
        public void SetCalConsistent(bool on) { Core.CalConsistent = on; }
        public void SetNoise(double scale) { Core.NoiseScale = scale; }
        public void SetCntPin(bool level) { Core.CntPin = level; }
        /// Brown-out: BORF and chip reset.
        public void BrownOut() { Core.Reset(Now); Core.ForceIntf(0x80); }

        public ulong Transfers { get; private set; }
        public int Range => Core.Range;
        public int Mode => Core.Mode;
        public string Role => Core.RoleName;
        public int Intf => Core.Intf;
        public int Ad1 => Core.Ad1Code;
        public bool CmpPin => Core.CmpPin;
        public string Status => string.Format("{0} ad1={1} lpf={2} rms={3} intf=0x{4:X2} cmp={5} period={6:F4}s",
            Core.RoleName, Core.Ad1Code, Core.LpfCode, Core.RmsValue, Core.Intf, Core.CmpPin, Core.Ad1Period);

        private double Now => machine.ElapsedVirtualTime.TimeElapsed.TotalSeconds;

        // ---- GPIO bit-bang ----
        public void OnGPIO(int number, bool value)
        {
            switch(number)
            {
            case 0: // SCK
                if(value && !sck) OnRising();
                else if(!value && sck) OnFalling();
                sck = value;
                break;
            case 1: // MOSI
                mosi = value;
                break;
            case 2: // CS (active low)
                if(!value && !selected)
                {
                    selected = true;
                    rising = 0; falling = 0; shift = 0; cmdDone = false; wbyte = 0;
                    if(Core == null) return; // pin level reported by Connect() in the constructor
                    Transfers++;
                    LoadCal();
                    Core.Update(Now);
                }
                else if(value && selected)
                {
                    selected = false;
                    DriveMiso(Core.Irq); // DO = IRQ while CS is high
                }
                break;
            }
        }

        private void OnRising()
        {
            if(!selected || Core == null) return;
            rising++;
            if(!cmdDone)
            {
                shift = (byte)((shift << 1) | (mosi ? 1 : 0));
                if(rising == 8)
                {
                    cmdDone = true;
                    reg = shift >> 1;
                    rd = (shift & 1) != 0;
                    shift = 0;
                    readIdx = -1;
                    this.Log(LogLevel.Debug, "{0} R{1:X2}", rd ? "read" : "write", reg);
                }
            }
            else if(!rd)
            {
                shift = (byte)((shift << 1) | (mosi ? 1 : 0));
                if(++wbyte == 8)
                {
                    this.Log(LogLevel.Debug, "  R{0:X2} <= 0x{1:X2}", reg, shift);
                    Core.Write(reg, shift, Now);
                    reg = (reg + 1) & 0x3F;
                    shift = 0;
                    wbyte = 0;
                }
            }
        }

        private void OnFalling()
        {
            if(!selected || Core == null) return;
            falling++;
            if(cmdDone && rd && falling >= 9)
            {
                var bit = falling - 9;
                var byteIdx = bit / 8;
                var bitIdx = 7 - (bit % 8);
                if(byteIdx != readIdx)
                {
                    readIdx = byteIdx;
                    curByte = Core.Read((reg + byteIdx) & 0x3F, Now);
                    this.Log(LogLevel.Noisy, "  R{0:X2} => 0x{1:X2}", (reg + byteIdx) & 0x3F, curByte);
                }
                DriveMiso(((curByte >> bitIdx) & 1) != 0);
            }
        }

        private void DriveMiso(bool b)
        {
            ((IGPIOReceiver)portC).OnGPIO(misoPin, b);
        }

        /// The meter's calibration record in flash (0x080C0000: crc, ~crc, version 1, data;
        /// DMM calibration at +0x168) is loaded once, on the first SPI transfer.
        private void LoadCal()
        {
            if(calLoaded) return;
            if(Now <= 0) return; // not before the machine runs (flash image not loaded yet)
            calLoaded = true;
            try
            {
                var hdr = machine.SystemBus.ReadBytes(CalRecord, 8);
                var version = BitConverter.ToUInt32(hdr, 4);
                if(version != 1)
                {
                    this.Log(LogLevel.Warning, "calibration record version {0}: using nominal gains", version);
                    return;
                }
                var raw = machine.SystemBus.ReadBytes(CalRecord + 0x168, 0x1B8);
                var cal = new float[0x1B8 / 4];
                for(var i = 0; i < cal.Length; i++) cal[i] = BitConverter.ToSingle(raw, 4 * i);
                Core.Cal = cal;
                this.Log(LogLevel.Info, "calibration loaded: model {0}, DC V gain {1:F5}, ohm offs {2}", raw[0], cal[1], cal[0x144 / 4]);
            }
            catch(Exception e)
            {
                this.Log(LogLevel.Warning, "calibration record not readable: {0}", e.Message);
            }
        }

        private const ulong CalRecord = 0x080C0000;

        private readonly IMachine machine;
        private readonly INumberedGPIOOutput portC;
        private readonly int misoPin;
        private bool selected, sck, mosi, cmdDone, rd, calLoaded;
        private int rising, falling, reg, wbyte, readIdx;
        private byte shift, curByte;
    }
}
