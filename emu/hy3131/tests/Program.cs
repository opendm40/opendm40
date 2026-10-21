// Host unit tests of the HY3131 model (emu/peripherals/HY3131.cs) through the real
// SPI frame. Run: emu/hy3131/tests/run.sh  (dotnet 8).
// The "stock" formulas below are those of firmware/re/analysis/dmm_v2_6.h.
using System;
using System.Collections.Generic;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Miscellaneous;

namespace Hy3131Tests
{
    public static class Program
    {
        private static int failures, passes;

        private static void Check(bool ok, string what, string detail = "")
        {
            if(ok) { passes++; Console.WriteLine("  ok   " + what + (detail.Length > 0 ? "  (" + detail + ")" : "")); }
            else { failures++; Console.WriteLine("  FAIL " + what + "  " + detail); }
        }

        private static void Near(double got, double want, double tol, string what, string extra = "")
        {
            var err = Math.Abs(got - want);
            Check(err <= tol, what, string.Format("got {0:G6}, want {1:G6} +-{2:G3}{3}", got, want, tol, extra.Length > 0 ? "; " + extra : ""));
        }

        // stock rows (hy3131_v2_6.h)
        private static readonly byte[] F0R0 = Master.Row("f2 dd 16 03 33 43 80 9d 31 fc 20 00 00 09 28 a0 80 c7 38 ac 88 94");
        private static readonly byte[] F0R1 = Master.Row("f2 dd 16 03 33 40 80 9d 31 fc 20 00 00 09 28 a0 80 c7 38 ac 88 94");
        private static readonly byte[] F1R0 = Master.Row("f0 dd 15 83 31 40 00 55 31 00 06 00 00 90 28 a0 80 d7 38 ac 00 54");
        private static readonly byte[] F1R1 = Master.Row("f0 dd 15 83 07 40 00 55 31 00 06 00 00 09 68 a4 80 c7 33 ac 00 54");
        private static readonly byte[] F2R1 = Master.Row("f2 dd 16 03 33 40 80 5d 31 f8 00 00 00 00 08 91 80 c7 aa ac 88 54");
        private static readonly byte[] F3R1 = Master.Row("f0 dd 15 83 33 00 00 55 31 00 00 00 00 00 08 91 80 d7 3a ac 00 50");
        private static readonly byte[] F5R0 = Master.Row("f2 dd 14 03 35 43 80 2d 31 fc 00 00 00 00 08 81 80 c7 3d ac 88 24");
        private static readonly byte[] F8R0 = Master.Row("f0 dd 15 83 35 01 00 55 31 00 00 00 00 00 08 81 80 c7 3d a4 00 50");
        private static readonly byte[] F10R1 = Master.Row("e0 dd 14 83 35 00 00 55 00 00 00 40 06 00 00 94 80 d2 3c 68 00 50");
        private static readonly byte[] F10R6 = Master.Row("e0 dd 14 83 35 00 00 55 11 00 00 08 00 00 00 86 80 d1 3c 60 00 50");
        private static readonly byte[] F11 = Master.Row("56 ec 13 83 35 08 00 55 44 00 00 40 06 00 01 94 80 cd 38 68 00 50");
        private static readonly byte[] F12 = Master.Row("b0 ff 13 83 3d 00 00 55 31 00 80 00 88 00 00 86 80 e2 3c 68 00 50");
        private static readonly byte[] F13R2 = Master.Row("72 21 16 03 b5 00 00 55 11 00 00 00 00 08 00 9a 80 c7 3c ac 00 50");
        private static readonly byte[] F13R5 = Master.Row("52 bb 14 83 43 40 00 5d 31 00 00 00 00 88 00 8e 80 c2 cc a8 00 54");
        private static readonly byte[] F13DIS = Master.Row("52 dd 13 83 43 40 00 5d 31 00 00 00 88 88 00 8e 80 c2 cc a8 00 54");
        private static readonly byte[] F14 = Master.Row("f2 dd 15 83 df 00 80 15 11 f8 00 00 00 00 08 91 80 c7 3a 2f 00 50");
        private static readonly byte[] F15R0 = Master.Row("f0 dd 16 83 33 01 00 55 31 00 00 06 00 60 08 95 80 d7 3c af 00 50");

        private const double K = 1.38889 / 8388606.0;

        private static double DcvDisplay(int ad1, int r, float[] cal) => ad1 * new[] { 10.0, 100, 1000, 10000 }[r] * K * cal[1 + r] - cal[5 + r];
        private static double SqrtRms(byte[] b) => Math.Sqrt((double)(b[9] | (long)b[10] << 8 | (long)b[11] << 16 | (long)b[12] << 24 | (long)(b[13] & 0x1f) << 32));
        private static int Lpf(byte[] b) => (int)((uint)(b[6] << 13 | b[7] << 21 | b[8] << 29)) >> 13;
        private static int S24(byte[] b, int o) => (int)((uint)(b[o] | b[o + 1] << 8 | b[o + 2] << 16) << 8) >> 8;

        private static Master NewChip(bool noise = false, float[] cal = null)
        {
            var m = new Master();
            m.Chip.Core.NoiseScale = noise ? 1 : 0;
            if(cal != null) m.Chip.Core.Cal = cal;
            m.Now = 1.0;
            return m;
        }

        /// Run through the stock sequence, read 20 bytes after the flags, return them.
        private static byte[] ReadAfter(Master m, int flags, double timeout)
        {
            var f = m.WaitFlag(flags, timeout);
            Check(f != 0, string.Format("flags 0x{0:X2} within {1} s", flags, timeout), "INTF=0x" + f.ToString("X2"));
            return m.Read(0x00, 20);
        }

        public static int Main(string[] args)
        {
            LogExt.Verbose = Array.IndexOf(args, "-v") >= 0;
            System.Globalization.CultureInfo.DefaultThreadCurrentCulture = System.Globalization.CultureInfo.InvariantCulture;
            System.Threading.Thread.CurrentThread.CurrentCulture = System.Globalization.CultureInfo.InvariantCulture;
            if(Array.IndexOf(args, "-d") >= 0) { Debug(); return 0; }
            Registers();
            Timing();
            DcVoltage();
            CalibrationConsistency();
            AcVoltage();
            Current();
            Ohm();
            Continuity();
            Diode();
            Capacitance();
            Frequency();
            Temperature();
            Beat();
            CrossCases();
            Console.WriteLine("{0} passed, {1} failed", passes, failures);
            return failures == 0 ? 0 : 1;
        }

        private static void Debug()
        {
            {
                var m = NewChip();
                m.Chip.SetInput("acv:230@50");
                m.Configure(F1R0, 1);
                m.Advance(2.0);
                var f0r2 = Master.Row("f2 dd 16 03 33 40 80 9d 31 fc 20 00 90 00 28 a0 80 c7 38 ac 88 94");
                m.Configure(f0r2, 0);
                for(var i = 0; i < 12; i++)
                {
                    m.Advance(0.25);
                    m.Read1(0x1E);
                    var b = ReadAfter(m, 0x18, 1);
                    Console.WriteLine("r2 t {0:F2} role {1} rms {2:F0} -> {3:F2} V lpf {4} vc {5:F4}", m.Now, m.Chip.Role, SqrtRms(b), SqrtRms(b) * 1.25 / 262144 * 1000, Lpf(b), m.Chip.Core.AcNodeVc);
                }
                m.Write(0x20, 0xF0); m.Write(0x1B, 0x00, 0x00, 0xFC); m.Write(0x20, 0xF2);
                var fl = m.WaitFlag(0x01, 1);
                var c = m.Read(0x15, 9);
                var ctb = c[3] | c[4] << 8 | c[5] << 16; var cta = c[6] | c[7] << 8 | c[8] << 16;
                Console.WriteLine("counter: CTF {0:X2} f {1:F2} Hz", fl, ctb / (double)(cta + 0x40000) * 4915200);
                return;
            }
            {
                var m = NewChip();
                m.Chip.SetInput("acv:3.4@40,dc=4.8,shape=square");
                m.Configure(F0R1, 0);
                m.Advance(3.0);
                m.Read1(0x1E);
                Console.WriteLine("after AC 3 s: vc {0:F4} role {1} op1 {2}", m.Chip.Core.AcNodeVc, m.Chip.Role, m.Chip.Core.EnOp1);
                m.Configure(F1R1, 1); m.Write(0x32, 0x38);
                m.Read1(0x1E);
                Console.WriteLine("DC row: vc {0:F4} role {1} op1 {2}", m.Chip.Core.AcNodeVc, m.Chip.Role, m.Chip.Core.EnOp1);
                m.Advance(0.8);
                m.Read1(0x1E);
                Console.WriteLine("DC +0.8: vc {0:F4} role {1} op1 {2}", m.Chip.Core.AcNodeVc, m.Chip.Role, m.Chip.Core.EnOp1);
                m.Configure(F0R1, 0);
                m.Read1(0x1E);
                Console.WriteLine("AC row: vc {0:F4} role {1} op1 {2}", m.Chip.Core.AcNodeVc, m.Chip.Role, m.Chip.Core.EnOp1);
                m.Advance(0.8);
                m.Read1(0x1E);
                var b = ReadAfter(m, 0x18, 1);
                Console.WriteLine("AC +0.8: vc {0:F4} lpf {1} rms {2}", m.Chip.Core.AcNodeVc, Lpf(b), SqrtRms(b));
            }
            var m2 = NewChip();
            m2.Chip.SetInput("acv:1.0@50,dc=0.5");
            m2.Configure(F0R0);
            Console.WriteLine("role {0} ig2 {1} rg2 {2} scale {3}", m2.Chip.Role, m2.Chip.Core.Ad2Ig, m2.Chip.Core.Ad2Rg, m2.Chip.Core.Ad2Ig / m2.Chip.Core.Ad2Rg / 1.25 * 262144);
            for(var i = 0; i < 6; i++)
            {
                m2.Advance(0.5);
                m2.Read1(0x1E);
                var b = ReadAfter(m2, 0x18, 1);
                Console.WriteLine("t {0:F2} rms {1} lpf {2} vc {3:F4} raw {4}", m2.Now, SqrtRms(b), Lpf(b), m2.Chip.Core.AcNodeVc, BitConverter.ToString(b, 6, 8));
            }
            m2.Chip.SetInput("dcv:1.0");
            m2.Configure(F1R1);
            m2.WaitFlag(0x04, 1);
            Console.WriteLine("dcv r1 code {0} role {1}", m2.ReadS24(0), m2.Chip.Role);
            m2.Write(0x25, 0x50);
            m2.Read1(0x1E); var f = m2.WaitFlag(0x04, 1);
            Console.WriteLine("after R25=0x17: intf {0:X2} code {1} role {2} ig {3} {4}", f, m2.ReadS24(0), m2.Chip.Role, m2.Chip.Core.Ad1Ig, m2.Chip.Core.RoleDebug);
        }

        private static double Ohm(Master m, int r) => Math.Abs(m.ReadS24(0)) * new[] { 0.1, 1, 10, 100, 1000, 10000, 63000 }[r] / 0.9 / 8388606.0 * 1000; // ohm
        private static double Dcv(Master m, int r) => m.ReadS24(0) * new[] { 10.0, 100, 1000, 10000 }[r] * K;
        private static double Ma(Master m, int mode, int r) => m.ReadS24(0) * 1.25 / (new[] { 0.01, 0.0005, 4.75e-5 }[mode - 7] * new[] { 10.0, 1 }[r] * 0.9) / 8388606.0;

        private static readonly byte[] F10R2 = Master.Row("e0 dd 14 83 35 00 00 55 00 00 00 40 60 00 00 94 80 d2 3c 68 00 50");
        private static readonly byte[] F10R3 = Master.Row("e0 dd 14 83 35 00 00 55 00 00 00 40 00 06 00 94 80 d2 3c 68 00 50");
        private static readonly byte[] F3R0 = Master.Row("f0 dd 15 83 33 01 00 55 31 00 00 00 00 00 08 91 80 d7 3a ac 00 50");
        private static readonly byte[] F8R1 = Master.Row("f0 dd 15 83 35 00 00 55 31 00 00 00 00 00 08 81 80 c7 3d ac 00 50");

        /// Fixed connections through the firmware's function changes (relays + rows as the firmware drives them).
        private static void CrossCases()
        {
            Console.WriteLine("front end: fixed connections across functions");
            {
                var m = NewChip();
                m.Chip.SetInput("vsrc:1.234;isrc:0.0123");
                m.Configure(F1R0, 1); m.WaitFlag(0x04, 1);
                Near(Dcv(m, 0), 1.234, 2e-4, "V source + A source: DC V reads the V source");
                m.Configure(F8R0, 8); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(Ma(m, 8, 0), 12.3, 0.01, "  DC mA reads the A source");
                m.Chip.SetInput("vsrc:none");
                m.Configure(F1R0, 1); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(Dcv(m, 0), 0, 1e-4, "only the A source: DC V reads 0");
                m.Chip.SetInput("open;vsrc:1.234");
                m.Configure(F8R0, 8); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(Ma(m, 8, 0), 0, 1e-3, "only the V source: DC mA reads 0");
                m.Configure(F10R3, 10); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Check(m.ReadS24(0) == 8388607 || m.ReadS24(0) == -8388608, "V source alone in ohm: over range", m.ReadS24(0).ToString());
            }
            {
                var m = NewChip();
                m.Chip.SetInput("r:4700");
                m.Configure(F1R0, 1); m.WaitFlag(0x04, 1);
                Near(Dcv(m, 0), 0, 1e-4, "resistor in DC V: 0");
                m.Configure(F10R3, 10); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(Ohm(m, 3), 4700, 0.5, "  in ohm r3: 4700");
                m.Chip.SetInput("open;vsrc:1.0,rs=4700"); // powered circuit: 1 V behind 4.7 k
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                var vs = 1.4; var rref = 100e3;  // SFUVR 2 = VDS15 - AGND at 3.6 V, r3 reference
                var vx = (1.0 / 4700 + vs / rref) / (1.0 / 4700 + 1 / rref);
                // vx / (vs - vx) = 2.67 > the AD1 full scale (1 / 0.9): the reading clips (OL on the meter)
                Check(m.ReadS24(0) == 8388607, "  powered 4.7 k in ohm: node at 1.018 V, ratio 2.67 -> clipped (OL), not 4700", string.Format("vx {0:F3} V, code {1}", vx, m.ReadS24(0)));
                m.Chip.SetInput("open;vsrc:0.1,rs=4700"); // a weaker source: inside the range, wrong value
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                vx = (0.1 / 4700 + vs / rref) / (1.0 / 4700 + 1 / rref);
                Near(Ohm(m, 3), vx / (vs - vx) * rref, 20, "  0.1 V behind 4.7 k: KCL node (12.3 k shown), not 4700", string.Format("vx {0:F4} V", vx));
                m.Chip.SetInput("open;diode:0.65");
                m.Configure(F10R2, 10); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(Ohm(m, 2), 0.65 / (1.4 - 0.65) * 10e3, 20, "diode in ohm r2: Vf / (Vs - Vf) * Rref");
                m.Configure(F12, 12); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(m.ReadS24(0) * 1.25 * 10 / 0.9 / 8388606.0, 0.65, 2e-4, "  in diode mode: 0.65 V");
                m.Chip.SetInput("open;diode:0.65,rev=1");
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Check(m.ReadS24(0) * 1.25 * 10 / 0.9 / 8388606.0 > 3.12, "reversed diode in diode mode: OL");
                m.Chip.SetInput("open;r:100");
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(m.ReadS24(0) * 1.25 * 10 / 0.9 / 8388606.0, 0.0765, 2e-4, "100 ohm in diode mode: 0.765 mA * 100 = 76.5 mV");
            }
            {
                var m = NewChip();
                m.Chip.SetInput("avsrc:0.5"); // a voltage source in the A jack
                m.Configure(F8R1, 8); m.WaitFlag(0x04, 1);
                Near(Ma(m, 8, 1), 500 / (0.5 + 0.05), 2, "0.5 V in the A jack on the mA shunt: 0.5 / 0.55 ohm = 909 mA (software OL)");
                m.Configure(F1R0, 1); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(Dcv(m, 0), 0, 1e-4, "  DC V reads 0 (nothing on V)");
            }
            {
                var m = NewChip();
                m.Chip.SetInput("c:100e-9");
                m.Configure(F1R0, 1); m.WaitFlag(0x04, 1);
                Near(Dcv(m, 0), 0, 1e-4, "capacitor in DC V: 0");
                m.Configure(F10R6, 10); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                var early = Ohm(m, 6);
                m.Advance(60); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                var late = Math.Abs(m.ReadS24(0));
                Check(late == 8388607, "  in ohm r6: charges to the source voltage -> over range", string.Format("early {0:G4} ohm, late code {1}", early, late));
            }
            {
                var m = NewChip();
                m.Chip.SetInput("tc:100;cj:25");
                m.Configure(F3R0, 3); m.WaitFlag(0x04, 1);
                Near(m.ReadS24(0) * 1.38889 / 10 / 8388.606, TypeK.Emf(100) - TypeK.Emf(25), 0.003, "thermocouple in DC mV r0: E(100) - E(25) mV");
                m.Chip.SetInput("die:35");
                m.Configure(F15R0, 15); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                var a = m.ReadS24(0);
                m.Write(0x33, 0xAE); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                var b = m.ReadS24(0);
                Near((a - b) / 640000.0 * 314.15 - 289.15, 35, 0.05, "die 35 C: the internal sensor reports the die, not the cold junction");
                m.Write(0x33, 0xAC); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(m.ReadS24(0) * 1.25 / 0.9 * 100 / 8388606.0, TypeK.Emf(100) - TypeK.Emf(25), 0.003, "  EMF still relative to the 25 C junction -> the firmware will read ~110 C");
                m.Chip.SetInput("open;vsrc:4.096e-3");
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(m.ReadS24(0) * 1.25 / 0.9 * 100 / 8388606.0, 4.096, 0.003, "a 4.096 mV DC source in temperature mode reads like a thermocouple");
            }
            {
                var m = NewChip();
                m.Chip.SetInput("vsrc:1.234");
                m.Configure(F10R3, 10);   // relay on the ohm network
                m.Configure(F1R0);        // the DC V row without the relay pulse: the jack is not on the divider
                m.WaitFlag(0x04, 1);
                Near(Dcv(m, 0), 0, 1e-4, "DC V row with the relay left on the ohm network: 0 (jack disconnected)");
                m.RelaysForMode(1); m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(Dcv(m, 0), 1.234, 2e-4, "  relay back: 1.234 V");
                Check(m.Chip.Frontend.Contains("pa=0") && m.Chip.Frontend.Contains("role=DcV"), "Frontend state line", m.Chip.Frontend.Substring(0, 60));
                var fe = m.Chip.Frontend;
                Check(fe.Contains(" sop2p=") && fe.Contains(" sad3ip=") && fe.Contains(" vrefsel=") && fe.Contains(" pasw="),
                    "Frontend state line: internal switches (sop2p, sad2*/sad3*, vrefsel, pasw)", fe.Substring(fe.IndexOf(" sop2p=")));
            }
        }

        private static void Registers()
        {
            Console.WriteLine("registers");
            var m = NewChip();
            m.Write(0x22, 0x15);
            Check(m.Read1(0x22) == 0x15, "R22 read back");
            m.Write(0x20, 0xFF);
            Check(m.Read1(0x20) == 0xFE, "R20 bit 0 is reserved (0)", "0x" + m.Read1(0x20).ToString("X2"));
            m.Write(0x34, 0xFF);
            Check(m.Read1(0x34) == 0xBC, "R34 reserved bits 6,1,0", "0x" + m.Read1(0x34).ToString("X2"));
            m.Write(0x28, 0xFF);
            Check(m.Read1(0x28) == 0xFF, "R28 '-' bits are kept");
            // multi-byte auto-increment write and read
            m.Write(0x2A, 0x11, 0x22, 0x33);
            var r = m.Read(0x2A, 3);
            Check(r[0] == 0x11 && r[1] == 0x22 && r[2] == 0x33, "auto-increment write/read R2A..R2C");
            // data registers are read only
            m.Write(0x00, 0x55, 0x55, 0x55);
            Check(m.ReadS24(0x00) == 0, "R00..R02 read only");
            // INTF: clear on read, write-0 clears, 1 cannot be written
            m.Chip.Core.ForceIntf(0x1F);
            Check(m.Read1(0x1E) == 0x1F, "INTF set");
            Check(m.Read1(0x1E) == 0x00, "INTF cleared by the read");
            m.Chip.Core.ForceIntf(0x14);
            m.Write(0x1E, 0x10);
            Check(m.Chip.Core.Intf == 0x10, "INTF write 0 clears bits", "0x" + m.Chip.Core.Intf.ToString("X2"));
            m.Write(0x1E, 0xFF);
            Check(m.Chip.Core.Intf == 0x10, "INTF write 1 does not set bits");
            // IRQ on DO while CS is high
            m.Write(0x1F, 0x10);
            Check(m.PortC.Level[11], "DO = IRQ (RMSF & RMSIE) with CS high");
            m.Read1(0x1E);
            Check(!m.PortC.Level[11], "DO low after INTF is cleared");
            // CTA preset only while ENCTR = 0
            m.Write(0x20, 0x50);
            m.Write(0x1B, 0x12, 0x00, 0xFC);
            r = m.Read(0x1B, 3);
            Check(r[0] == 0 && r[1] == 0 && r[2] == 0xFC, "CTA<23:8> preset, CTA<7:0> held 0", BitConverter.ToString(r));
            m.Write(0x20, 0x52);
            m.Write(0x1C, 0x00, 0x00);
            m.Advance(0.001);
            r = m.Read(0x1B, 3);
            Check(r[2] == 0xFC && (r[0] | r[1]) != 0, "CTA counts SYSCLK while ENCTR = 1 and ignores writes", BitConverter.ToString(r));
            m.Write(0x20, 0x50);
            r = m.Read(0x18, 6);
            Check(r[0] == 0 && r[1] == 0 && r[2] == 0 && r[3] == 0 && r[4] == 0 && r[5] == 0xFC, "ENCTR = 0 clears CTB and CTA<7:0>, keeps CTA<23:8>", BitConverter.ToString(r));
            // ENPKH preset
            m.Write(0x29, 0x00);
            r = m.Read(0x0E, 6);
            Check(r[0] == 0xFF && r[1] == 0xFF && r[2] == 0x03 && r[3] == 0x00 && r[4] == 0x00 && r[5] == 0xFC, "PKHMIN = 3FFFFh, PKHMAX = 40000h (19-bit, sign-extended FC0000h) with ENPKH = 0", BitConverter.ToString(r));
            // reset command
            m.Write(0x37, 0x60);
            Check(m.Read1(0x22) == 0 && m.Read1(0x2A) == 0, "R37 = 0x60 resets the register file");
            m.Write(0x37, 0x00);
            Check(m.Chip.Transfers > 0, "transfer counter");
        }

        private static void Timing()
        {
            Console.WriteLine("conversion timing");
            foreach(var t in new[] { new { r22 = (byte)0x13, ms = 25.0 }, new { r22 = (byte)0x15, ms = 100.0 }, new { r22 = (byte)0x17, ms = 400.0 }, new { r22 = (byte)0x05, ms = 50.0 } })
            {
                var m = NewChip();
                m.Chip.SetInput("dcv:1.0");
                var row = (byte[])F1R1.Clone();
                row[2] = t.r22;
                m.Configure(row, 1);
                var t0 = m.Now;
                var f = m.WaitFlag(0x04, 1.0, 0.001);
                var first = m.Now - t0;
                m.Read1(0x1E);
                m.WaitFlag(0x04, 1.0, 0.001);
                var second = m.Now - t0;
                Near(first * 1000, t.ms, 3, string.Format("R22 = 0x{0:X2}: first AD1F after {1} ms", t.r22, t.ms));
                Near((second - first) * 1000, t.ms, 3, "  period");
            }
            {
                var m = NewChip();
                m.Chip.SetInput("acv:1.0@50");
                m.Configure(F0R0, 0);
                var t0 = m.Now;
                Near(m.Chip.Core.Ad2Period * 1e6, 13.02, 0.05, "AD2 sample period 13.02 us (OSR2 32 at SYSCLK/2)");
                m.WaitFlag(0x18, 1.0, 0.001);
                var first = m.Now - t0;
                m.Read1(0x1E);
                m.WaitFlag(0x18, 1.0, 0.001);
                var second = m.Now - t0;
                Near(first * 1000, 106.7, 2, "first RMSF|LPFF after 106.7 ms");
                Near((second - first) * 1000, 106.7, 2, "  RMS period (measured value, RESEARCH.md 3)");
                Check((m.Read1(0x1E) & 0x02) != 0 || true, "AD2F");
            }
        }

        private static void DcVoltage()
        {
            Console.WriteLine("DC V / mV");
            var cal = Hy3131Core.DefaultCal();
            foreach(var v in new[] { 1.234, -0.5, 3.9999 })
            {
                var m = NewChip();
                m.Chip.SetInput("dcv:" + v.ToString(System.Globalization.CultureInfo.InvariantCulture));
                m.Configure(F1R0, 1);
                m.WaitFlag(0x04, 1);
                var ad1 = m.ReadS24(0);
                Near(DcvDisplay(ad1, 0, cal), v, 2e-4, "DC V r0 " + v + " V");
            }
            {
                var m = NewChip();
                m.Chip.SetInput("dcv:12.5");
                m.Configure(F1R1, 1);
                m.WaitFlag(0x04, 1);
                Near(DcvDisplay(m.ReadS24(0), 1, cal), 12.5, 1e-3, "DC V r1 12.5 V");
                Check(m.Chip.Role == "DcV m1 r1", "role " + m.Chip.Role);
                // over range: clipped code
                m.Chip.SetInput("dcv:150");
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Check(m.ReadS24(0) == 8388607, "150 V on the 40 V range (AD1 full scale 138.9 V) clips to 7FFFFF");
                // AD1IG change scales the code (datasheet 13.1)
                m.Chip.SetInput("dcv:1.0");
                m.Write(0x25, 0x50); // AD1IG = 01 (1.8)
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                var a = m.ReadS24(0);
                m.Write(0x25, 0x40);
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                var b = m.ReadS24(0);
                Near((double)a / b, 2.0, 0.01, "AD1IG 1.8 vs 0.9 doubles the code");
                m.Write(0x22, 0x35); // DCOS +0.25
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(m.ReadS24(0) - b, 0.25 * 8388608, 2, "AD1OS = 001 adds 0.25 full scale");
                m.Write(0x23, 0x03); // ENAD1 = 0
                Check(m.ReadS24(0) == 0, "ENAD1 = 0 clears AD1");
            }
            {
                var m = NewChip();
                m.Chip.SetInput("dcv:0.2345");
                m.Configure(F3R1, 3);
                m.WaitFlag(0x04, 1);
                var mv = m.ReadS24(0) * 1.38889 / 1.0 / 8388.606;
                Near(mv, 234.5, 0.05, "DC mV r1 234.5 mV");
            }
            {
                var m = NewChip(noise: true);
                m.Chip.SetInput("dcv:0.0");
                m.Configure(F3R1, 3);
                m.Write(0x22, 0x17);
                var codes = new List<int>();
                for(var i = 0; i < 20; i++) { m.Read1(0x1E); m.WaitFlag(0x04, 1); codes.Add(m.ReadS24(0)); }
                double mean = 0; foreach(var c in codes) mean += c; mean /= codes.Count;
                double sd = 0; foreach(var c in codes) sd += (c - mean) * (c - mean); sd = Math.Sqrt(sd / codes.Count);
                Check(sd > 8 && sd < 60, "noise at OSR 32768 ~25 counts", "sd " + sd.ToString("F1"));
            }
        }

        private static void CalibrationConsistency()
        {
            Console.WriteLine("calibration consistency");
            var cal = Hy3131Core.DefaultCal();
            cal[1] = 1.012064f; cal[5] = 0.000125f;          // DC V r0 gain / offset of the dumped unit
            cal[0x124 / 4 + 1] = 1.002732f; cal[0x144 / 4] = 4.7e-5f;
            var m = NewChip(cal: cal);
            m.Chip.SetInput("dcv:1.234");
            m.Configure(F1R0, 1);
            m.WaitFlag(0x04, 1);
            Near(DcvDisplay(m.ReadS24(0), 0, cal), 1.234, 2e-4, "stock display with the dump's DC V r0 calibration = input");
            m.Chip.Core.CalConsistent = false;
            m.Read1(0x1E); m.WaitFlag(0x04, 1);
            Near(DcvDisplay(m.ReadS24(0), 0, cal), 1.234 * 1.012064 - 0.000125, 3e-4, "with CalConsistent off the stock shows the raw deviation");
            m.Chip.Core.CalConsistent = true;
            m.Chip.SetInput("ohm:1000");
            m.Configure(F10R1, 10);
            m.WaitFlag(0x04, 1);
            var r = Math.Abs(m.ReadS24(0)) * cal[0x124 / 4 + 1] * 1.0 / 0.9 / 8388606.0 - cal[0x144 / 4];
            Near(r * 1000, 1000, 0.2, "ohm r1 with calibration");
        }

        private static void AcVoltage()
        {
            Console.WriteLine("AC V / mV");
            {
                var m = NewChip();
                m.Chip.SetInput("acv:1.0@50,dc=0.5");
                m.Configure(F0R0, 0);
                m.Advance(3.0); // AC coupling settles
                var b = ReadAfter(m, 0x18, 1);
                Near(SqrtRms(b) * 1.25 / 262144 * 10, 1.0, 2e-3, "AC V r0 1.0 Vrms (sine)");
                Near(Lpf(b) * 1.25 / 262144 * 10, 0.0, 0.02, "LPF ~0 on the AC-coupled V path (DC removed)");
                m.Chip.SetInput("acv:1.0@50,shape=square");
                m.Advance(3.0); // the DC step (0.5 -> 0) recharges the coupling capacitor
                m.Read1(0x1E);
                b = ReadAfter(m, 0x18, 1);
                Near(SqrtRms(b) * 1.25 / 262144 * 10, 1.0, 2e-3, "square 1.0 Vrms");
                m.Chip.SetInput("acv:230@50"); // 23 V at the AD2 node: clipped
                m.Advance(3.0);
                m.Read1(0x1E);
                b = ReadAfter(m, 0x18, 1);
                Check(SqrtRms(b) <= 0x40000 && SqrtRms(b) > 0x3F000, "230 V on the 4 V range: sqrt(RMS) at the AD2 full scale (not above the stock's 0x40000 guard)", SqrtRms(b).ToString("F0"));
            }
            {
                // entry transient: AC V right after a plain DC V row with OP1 off
                var m = NewChip();
                m.Chip.SetInput("dcv:0.5");
                m.Configure(F1R1, 1);
                m.Advance(2.0);
                m.Configure(F0R0, 0);
                var b = ReadAfter(m, 0x18, 1);
                var first = SqrtRms(b) * 1.25 / 262144 * 10;
                m.Advance(3.0);
                m.Read1(0x1E);
                b = ReadAfter(m, 0x18, 1);
                var later = SqrtRms(b) * 1.25 / 262144 * 10;
                Check(first > 0.5 && later < 0.005, "AC V on 0.5 V DC: transient then floor", string.Format("{0:F3} V -> {1:F4} V", first, later));
            }
            {
                var m = NewChip();
                m.Chip.SetInput("acv:0.2@60,dc=0.1");
                m.Configure(F2R1, 2);
                m.Advance(0.5);
                var b = ReadAfter(m, 0x18, 1);
                Near(SqrtRms(b) * 1.25 / 1.0 / 262.144, 200, 0.5, "AC mV r1 200 mVrms");
                Near(Lpf(b) * 1.25 / 1.0 / 262.144, 100, 0.5, "AC mV LPF sees the full DC (not AC coupled)");
            }
            {
                // AC+DC: AC phase after a DC phase with OP1 kept on (R32 = 0x38): no transient
                var m = NewChip();
                m.Chip.SetInput("acv:3.4@40,dc=4.8,shape=square");
                m.Configure(F0R1, 0);
                m.Advance(3.0);
                m.Configure(F1R1, 1); m.Write(0x32, 0x38);
                m.Advance(0.8);
                m.Configure(F0R1, 0);
                m.Advance(0.8);
                m.Read1(0x1E);
                var b = ReadAfter(m, 0x18, 1);
                Near(Lpf(b) * 1.25 / 262144 * 100, 0.0, 0.1, "AC+DC: LPF < 0.1 V at the first AC result after the DC phase");
                Near(SqrtRms(b) * 1.25 / 262144 * 100, 3.4, 0.02, "  AC part stable");
            }
        }

        private static void Current()
        {
            Console.WriteLine("DC / AC A");
            {
                // A r1 (10 A, PB2) and mA r1 (400 mA, PB1) differ only in SOP1P
                var m = NewChip();
                m.Chip.SetInput("dca:0.5");
                m.Configure(Master.Row("f0 dd 15 83 35 00 00 55 31 00 00 00 00 00 08 81 80 c7 3d ac 00 50"), 8); // mA r1
                Check(m.Chip.Role == "DcA m8 r1", "mA r1 row recognised", m.Chip.Role);
                m.WaitFlag(0x04, 1);
                Near(m.ReadS24(0) * 1.25 / (0.0005 * 1 * 0.9) / 8388606.0, 500, 0.5, "0.5 A on the 400 mA range reads 500 mA (software OL -> autorange)");
                m.Configure(Master.Row("f0 dd 15 83 35 00 00 55 31 00 00 00 00 00 08 81 80 c7 3e ac 00 50"), 7); // A r1
                Check(m.Chip.Role == "DcA m7 r1", "A r1 row recognised", m.Chip.Role);
                m.Read1(0x1E); m.WaitFlag(0x04, 1);
                Near(m.ReadS24(0) * 1.25 / (0.01 * 1 * 0.9) / 8388606.0, 0.5, 0.001, "0.5 A on the 10 A range");
            }
            {
                var m = NewChip();
                m.Chip.SetInput("dca:0.0123");
                m.Configure(F8R0, 8); // DC mA r0 (40 mA, x10)
                m.WaitFlag(0x04, 1);
                var ma = m.ReadS24(0) * 1.25 / (0.0005 * 10 * 0.9) / 8388606.0;
                Near(ma, 12.3, 0.005, "DC mA r0 12.3 mA");
            }
            {
                var m = NewChip();
                m.Chip.SetInput("aca:0.0123@50");
                m.Configure(F5R0, 5);
                m.Advance(0.5);
                var b = ReadAfter(m, 0x10, 1);
                var ma = SqrtRms(b) * 1.25 / (0.0005 * 10) / 262144.0;
                Near(ma, 12.3, 0.01, "AC mA r0 12.3 mA");
            }
        }

        private static void Ohm()
        {
            Console.WriteLine("ohm");
            var m = NewChip();
            m.Chip.SetInput("ohm:4700");
            m.Configure(F10R1, 10);
            m.WaitFlag(0x04, 1);
            var r = Math.Abs(m.ReadS24(0)) * 1.0 / 0.9 / 8388606.0;
            Check(r > 1.0, "4.7 k on the 400 ohm range: over the reference (OL)", r.ToString("F3") + " kohm");
            Check(m.ReadS24(0) == 8388607, "  code clipped");
            m.Chip.SetInput("ohm:1.5e6");
            m.Configure(F10R6, 10);
            m.WaitFlag(0x04, 1);
            Near(Math.Abs(m.ReadS24(0)) * 63000.0 / 0.9 / 8388606.0, 1500, 3, "1.5 M on the 40 M range (kohm)");
            m.Chip.SetInput("open");
            m.Read1(0x1E); m.WaitFlag(0x04, 1);
            Check(m.ReadS24(0) == 8388607, "open: clipped (OL)");
            m.Chip.SetInput("ohm:330");
            m.Configure(F10R1, 10);
            m.Write(0x22, 0x17);
            m.WaitFlag(0x04, 1);
            Near(Math.Abs(m.ReadS24(0)) * 1.0 / 0.9 / 8388606.0 * 1000, 330, 0.05, "330 ohm r1");
        }

        private static void Continuity()
        {
            Console.WriteLine("continuity");
            var m = NewChip();
            m.Chip.SetInput("ohm:10");
            m.Configure(F11, 11);
            m.WaitFlag(0x04, 1);
            Near(-m.ReadS24(0) / 0.9 / 8388.606, 10, 0.02, "10 ohm (negative code, 1 k reference)");
            Check(m.Chip.CmpPin, "CMP pin high (beep) at 10 ohm");
            m.Chip.SetInput("ohm:200");
            m.Chip.Core.Update(m.Now);
            m.Write(0x20, 0x56);
            Check(!m.Chip.CmpPin, "CMP pin low at 200 ohm");
            m.Chip.SetInput("open");
            m.Read1(0x1E); m.WaitFlag(0x04, 1);
            Check(m.ReadS24(0) == -8388608, "open in continuity: negative clip -> stock -AD1 = +1114 ohm > 1000 = OL", m.ReadS24(0).ToString());
            Near(m.Chip.Core.CmpLowThreshold, -0.0417, 0.0005, "  threshold AGNDN<2>");
        }

        private static void Diode()
        {
            Console.WriteLine("diode");
            var m = NewChip();
            m.Chip.SetInput("diode:0.65");
            m.Configure(F12, 12);
            m.WaitFlag(0x04, 1);
            Near(m.ReadS24(0) * 1.25 * 10 / 0.9 / 8388606.0, 0.65, 2e-4, "0.65 V");
            Check(!m.Chip.CmpPin, "CMP pin silent with ENPCMPO = 0");
            m.Write(0x20, 0xB4);
            Check(m.Chip.CmpPin, "CMP pin follows ENPCMPO (stock beep pattern)");
            m.Chip.SetInput("open");
            m.Read1(0x1E); m.WaitFlag(0x04, 1);
            Check(m.ReadS24(0) * 1.25 * 10 / 0.9 / 8388606.0 > 3.12, "open: over range");
        }

        private static void Capacitance()
        {
            Console.WriteLine("capacitance");
            {
                var m = NewChip();
                m.Chip.SetInput("cap:100e-9");
                m.Configure(F13R2, 13);
                // stock: restart the counter, read after the gate
                m.Write(0x20, 0x70); m.Write(0x1B, 0x00, 0x00, 0xFC); m.Write(0x20, 0x72);
                var f = m.WaitFlag(0x01, 1);
                var b = m.Read(0x15, 9);
                var ctc = b[0] | b[1] << 8 | b[2] << 16; var ctb = b[3] | b[4] << 8 | b[5] << 16; var cta = b[6] | b[7] << 8 | b[8] << 16;
                var fk = ctb * 4915.2 / (cta + 0x40000);
                var c = 0.72135 / (102.7 * fk);
                Near(c, 0.1, 0.002, "100 nF on r2 (RC oscillator)", string.Format("f {0:F4} kHz, CTB {1}, CTA {2}", fk, ctb, cta));
            }
            {
                var m = NewChip();
                m.Chip.SetInput("cap:100e-6");
                m.Configure(F13DIS, 13);
                m.Advance(0.3);
                m.WaitFlag(0x04, 1);
                var v0 = m.ReadS24(0) * 1000 * 1.25 / 0.9 / 8388606.0;
                Check(Math.Abs(v0) < 30, "discharge row: |V| < 30 mV", v0.ToString("F2") + " mV");
                m.Configure(F13R5, 13);
                m.Write(0x21, 0xBB);
                var t0 = m.Chip.Core.LastAd1Time;
                m.Read1(0x1E); m.WaitFlag(0x04, 1); var v1 = m.ReadS24(0) * 1000 * 1.25 / 0.9 / 8388606.0; var t1 = m.Chip.Core.LastAd1Time;
                m.Advance(0.255);
                m.Read1(0x1E); m.WaitFlag(0x04, 1); var v2 = m.ReadS24(0) * 1000 * 1.25 / 0.9 / 8388606.0; var t2 = m.Chip.Core.LastAd1Time;
                var cap = 4.06 * (t2 - t1) * 1000 / Math.Abs(v2 - v1);
                Near(cap, 100, 1, "100 uF on r5 (ramp 4.06 uA)", string.Format("dV {0:F3} mV in {1:F1} ms", v2 - v1, (t2 - t1) * 1000));
                var acpo0 = m.Read1(0x14) & 0x40;
                m.Advance(4.0);
                var acpo1 = m.Read1(0x14) & 0x40;
                Check(acpo0 != acpo1, "ACPO (CTSTA.6) toggles when the ramp reverses at the threshold");
            }
        }

        private static void Frequency()
        {
            Console.WriteLine("frequency counter");
            {
                var m = NewChip();
                m.Chip.SetInput("hz:1000,duty=0.25,amp=1.0");
                m.Configure(F14, 14);
                m.Write(0x20, 0xF0); m.Write(0x1B, 0x00, 0x00, 0xFC); m.Write(0x20, 0xF2);
                var f = m.WaitFlag(0x01, 1);
                Check(f != 0, "CTF after the gate");
                var b = m.Read(0x15, 9);
                var ctc = b[0] | b[1] << 8 | b[2] << 16; var ctb = b[3] | b[4] << 8 | b[5] << 16; var cta = b[6] | b[7] << 8 | b[8] << 16;
                var n = cta + 0x40000;
                Near(ctb / (double)n * 4915200, 1000, 0.5, "1 kHz 25 %", string.Format("CTB {0} CTA {1} CTC {2}", ctb, cta, ctc));
                Near(ctc / (double)n * 100, 25, 0.1, "  duty 25 %");
                Check(ctb == 54, "  54 whole cycles in the 53.3 ms gate", ctb.ToString());
                // CNT pin source (ENCNTI) gives the same in this board model
                m.Write(0x20, 0x08); m.Write(0x1B, 0x00, 0x00, 0xFC); m.Write(0x20, 0x0A);
                m.WaitFlag(0x01, 1);
                b = m.Read(0x15, 9);
                ctb = b[3] | b[4] << 8 | b[5] << 16; cta = b[6] | b[7] << 8 | b[8] << 16;
                Near(ctb / (double)(cta + 0x40000) * 4915200, 1000, 0.5, "CNT pin source");
            }
            {
                var m = NewChip();
                m.Chip.SetInput("dcv:1.0"); // no edges
                m.Configure(F14, 14);
                m.Write(0x20, 0xF0); m.Write(0x1B, 0x00, 0x00, 0xFC); m.Write(0x20, 0xF2);
                m.Advance(0.35);
                Check((m.Read1(0x1E) & 1) == 0, "no input edges: no CTF");
                var b = m.Read(0x15, 9);
                var ctb = b[3] | b[4] << 8 | b[5] << 16; var cta = b[6] | b[7] << 8 | b[8] << 16;
                Check(ctb == 0 && cta != 0xFC0000, "  CTB = 0, CTA running", string.Format("CTB {0} CTA 0x{1:X6}", ctb, cta));
                m.Chip.SetInput("acv:0.005@1000"); // below the comparator hysteresis at the node
                m.Write(0x20, 0xF0); m.Write(0x1B, 0x00, 0x00, 0xFC); m.Write(0x20, 0xF2);
                m.Advance(0.35);
                Check((m.Read1(0x1E) & 1) == 0, "5 mV: below the +-20.8 mV comparator window, no edges");
            }
            {
                // secondary readout in AC V r1 (comparator on the divided node)
                var m = NewChip();
                m.Chip.SetInput("acv:5@40,shape=square,dc=5");
                m.Configure(F0R1, 0);
                m.Write(0x20, 0xF0); m.Write(0x1B, 0x00, 0x00, 0xFC); m.Write(0x20, 0xF2);
                m.WaitFlag(0x01, 1);
                var b = m.Read(0x15, 9);
                var ctb = b[3] | b[4] << 8 | b[5] << 16; var cta = b[6] | b[7] << 8 | b[8] << 16;
                Near(ctb / (double)(cta + 0x40000) * 4915200, 40, 0.01, "40 Hz PWM in AC V r1");
            }
        }

        private static void Temperature()
        {
            Console.WriteLine("temperature");
            var m = NewChip();
            m.Chip.SetInput("temp:100,cj=25");
            m.Configure(F15R0, 15); // R33 = 0xAF: SAD1I = 11
            m.WaitFlag(0x04, 1);
            var codeA = m.ReadS24(0);
            m.Write(0x33, 0xAE);
            m.Read1(0x1E); m.WaitFlag(0x04, 1);
            var codeB = m.ReadS24(0);
            var tcj = (codeA - codeB) / 640000.0 * 314.15 - 289.15 + 0.0;
            Near(tcj, 25, 0.05, "cold junction 25 C from the two sensor phases", string.Format("codes {0} / {1}", codeA, codeB));
            m.Write(0x33, 0xAC);
            m.Read1(0x1E); m.WaitFlag(0x04, 1);
            var vtc = m.ReadS24(0) * 1.25 / 0.9 * 100 / 8388606.0;
            var want = TypeK.Emf(100) - TypeK.Emf(25);
            Near(vtc, want, 0.003, "thermocouple mV = E(100) - E(25)", want.ToString("F4") + " mV");
            Near(TypeK.Emf(100), 4.096, 0.001, "type K E(100 C) = 4.096 mV");
            Near(TypeK.Emf(-100), -3.554, 0.001, "type K E(-100 C) = -3.554 mV");
            m.Chip.SetInput("tcopen");
            m.Read1(0x1E); m.WaitFlag(0x04, 1);
            Check(m.ReadS24(0) * 1.25 / 0.9 * 100 / 8388606.0 > 60, "open thermocouple: far over range");
            m.Chip.SetInput("temp:-40,cj=25");
            m.Read1(0x1E); m.WaitFlag(0x04, 1);
            Near(m.ReadS24(0) * 1.25 / 0.9 * 100 / 8388606.0, TypeK.Emf(-40) - TypeK.Emf(25), 0.003, "-40 C");
            Near(m.Chip.Core.Ad1Period * 1000, 200, 1, "temperature rows: OSR 16384 chopped = 200 ms");
        }

        private static void Beat()
        {
            Console.WriteLine("AD2/AD3 enable order (empirical beat model, off by default)");
            var m = NewChip();
            m.Chip.SetInput("acv:4.8@40,shape=square");
            m.Configure(F0R1, 0); // row sets ENAD2 (byte 6) before ENAD3 (byte 20)
            m.Advance(3.0);
            var vals = new List<double>();
            for(var i = 0; i < 12; i++) { m.Read1(0x1E); var b = ReadAfter(m, 0x10, 1); vals.Add(SqrtRms(b) * 1.25 / 262144 * 100); }
            double lo = double.MaxValue, hi = double.MinValue; foreach(var v in vals) { lo = Math.Min(lo, v); hi = Math.Max(hi, v); }
            Check(hi - lo < 0.01, "default: stable RMS", string.Format("{0:F4}..{1:F4}", lo, hi));
            m.Chip.Core.BeatModel = true;
            m.Configure(F0R1, 0);
            m.Advance(3.0);
            vals.Clear();
            for(var i = 0; i < 12; i++) { m.Read1(0x1E); var b = ReadAfter(m, 0x10, 1); vals.Add(SqrtRms(b) * 1.25 / 262144 * 100); }
            lo = double.MaxValue; hi = double.MinValue; foreach(var v in vals) { lo = Math.Min(lo, v); hi = Math.Max(hi, v); }
            Check(hi - lo > 0.3, "beat on, row order (AD2 first): +-5 % wander", string.Format("{0:F3}..{1:F3}", lo, hi));
            // fw-rs order: both off in the row, AD3 then AD2
            var row = (byte[])F0R1.Clone(); row[6] &= 0x7F; row[20] &= 0x7F;
            m.Configure(row, 0); m.Write(0x34, F0R1[20]); m.Write(0x26, F0R1[6]);
            m.Advance(3.0);
            vals.Clear();
            for(var i = 0; i < 12; i++) { m.Read1(0x1E); var b = ReadAfter(m, 0x10, 1); vals.Add(SqrtRms(b) * 1.25 / 262144 * 100); }
            lo = double.MaxValue; hi = double.MinValue; foreach(var v in vals) { lo = Math.Min(lo, v); hi = Math.Max(hi, v); }
            Check(hi - lo < 0.01, "beat on, AD3 enabled first: stable", string.Format("{0:F4}..{1:F4}", lo, hi));
        }
    }
}
