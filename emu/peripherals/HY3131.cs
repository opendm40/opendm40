//
// HY3131 DMM front end on its bit-banged SPI (SCK PC10, MOSI PC12, MISO PC11,
// CS PD2; firmware/re/analysis/hy3131_v2_6.h, fw-rs/src/hy3131.rs). Register
// level stub: a 64-byte register file the firmware writes (R20..R35 mode
// configuration), INTF (R1E) always reporting AD1F|AD2F|LPFF|RMSF|CTF so both
// DMM loops run at their own pace, and conversion results computed from a
// scripted input: SetAd1(raw) for a raw signed 24-bit AD1 code, SetVin(volts)
// for a DC voltage mapped through the stock DC V scale of the range whose
// configuration row (0x0804A320 + 22*r in the stock image) is currently
// loaded (falls back to range 0).
// Frame: cmd = (reg << 1) | rd, MSB first; a read has one extra SCK pulse
// after the command, then MISO is sampled with SCK low before each pulse.
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class HY3131 : IDoubleWordPeripheral, IKnownSize, IGPIOReceiver
    {
        public HY3131(IMachine machine, INumberedGPIOOutput portC, INumberedGPIOOutput portD,
                      int sck = 10, int miso = 11, int mosi = 12, int cs = 2)
        {
            this.machine = machine;
            this.portC = portC;
            this.misoPin = miso;
            portC.Connections[sck].Connect(this, 0);
            portC.Connections[mosi].Connect(this, 1);
            portD.Connections[cs].Connect(this, 2);
            Reset();
        }

        public void Reset()
        {
            Array.Clear(regs, 0, regs.Length);
            selected = false; sck = false; mosi = false;
            rising = 0; falling = 0; shift = 0; cmdDone = false; reg = 0; rd = false; wbyte = 0;
            Transfers = 0;
        }

        public long Size => 0x100;
        public uint ReadDoubleWord(long offset) => offset == 0 ? (uint)ad1 : 0;
        public void WriteDoubleWord(long offset, uint value) { }

        // ---- scripting ----
        public void SetAd1(int raw)
        {
            ad1 = raw;
            vinSet = false;
        }

        public void SetVin(double volts)
        {
            vin = volts;
            vinSet = true;
        }

        public void SetAd2(int raw) { ad2 = raw; }

        public ulong Transfers { get; private set; }
        public int Range => CurrentDcvRange();

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
                    Transfers++;
                }
                else if(value && selected)
                {
                    selected = false;
                }
                break;
            }
        }

        private void OnRising()
        {
            if(!selected) return;
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
                    if(rd)
                    {
                        PrepareRead();
                    }
                    this.Log(LogLevel.Debug, "{0} R{1:X2}", rd ? "read" : "write", reg);
                }
            }
            else if(!rd)
            {
                shift = (byte)((shift << 1) | (mosi ? 1 : 0));
                if(++wbyte == 8)
                {
                    this.Log(LogLevel.Debug, "  R{0:X2} <= 0x{1:X2}", reg, shift);
                    WriteReg(reg, shift);
                    reg = (reg + 1) & 0x3F;
                    shift = 0;
                    wbyte = 0;
                }
            }
        }

        private void OnFalling()
        {
            if(!selected) return;
            falling++;
            if(cmdDone && rd && falling >= 9)
            {
                var bit = falling - 9;
                var byteIdx = bit / 8;
                var bitIdx = 7 - (bit % 8);
                var v = ReadReg((reg + byteIdx) & 0x3F);
                var b = ((v >> bitIdx) & 1) != 0;
                this.Log(LogLevel.Noisy, "  miso bit {0} of R{1:X2}=0x{2:X2} -> {3}", bitIdx, (reg + byteIdx) & 0x3F, v, b);
                ((IGPIOReceiver)portC).OnGPIO(misoPin, b);
            }
        }

        private void PrepareRead()
        {
            // Snapshot conversion results so a multi-byte read is consistent.
            var a1 = CurrentAd1();
            regs[0x00] = (byte)a1; regs[0x01] = (byte)(a1 >> 8); regs[0x02] = (byte)(a1 >> 16);
            regs[0x03] = (byte)ad2; regs[0x04] = (byte)(ad2 >> 8); regs[0x05] = (byte)(ad2 >> 16);
            // LPF (R06..R08, 19-bit at bits 13..31) and RMS (R09..R0D): derived from AD2.
            var lpf = (long)ad2 << 13;
            regs[0x06] = (byte)lpf; regs[0x07] = (byte)(lpf >> 8); regs[0x08] = (byte)(lpf >> 16);
            var rms = (ulong)((long)ad2 * ad2);
            for(var i = 0; i < 5; i++) regs[0x09 + i] = (byte)(rms >> (8 * i));
            regs[0x1E] = 0x1F; // INTF: AD1F AD2F LPFF RMSF CTF
        }

        private byte ReadReg(int r)
        {
            return regs[r];
        }

        private void WriteReg(int r, byte v)
        {
            regs[r] = v;
        }

        private int CurrentAd1()
        {
            if(!vinSet) return ad1;
            // DC V: V = AD1 * acv_k[r] * 1.38889 / 8388606 (gain 1, offset 0)
            var r = CurrentDcvRange();
            var k = AcvK[r];
            var raw = vin * 8388606.0 / (1.38889 * k);
            return (int)Math.Max(-8388607, Math.Min(8388607, Math.Round(raw)));
        }

        /// Match R20..R35 against the stock DC V rows in flash; -1 = no match.
        private int CurrentDcvRange()
        {
            for(var r = 0; r < 4; r++)
            {
                var row = machine.SystemBus.ReadBytes(DcvRows + (ulong)(22 * r), 22);
                var same = true;
                for(var i = 0; i < 22 && same; i++)
                {
                    if(row[i] != regs[0x20 + i]) same = false;
                }
                if(same) return r;
            }
            return 0;
        }

        private const ulong DcvRows = 0x0804A320;
        private static readonly double[] AcvK = { 10.0, 100.0, 1000.0, 10000.0 };

        private readonly IMachine machine;
        private readonly INumberedGPIOOutput portC;
        private readonly int misoPin;
        private readonly byte[] regs = new byte[64];
        private bool selected, sck, mosi, cmdDone, rd, vinSet;
        private int rising, falling, reg, wbyte;
        private byte shift;
        private int ad1 = 0, ad2 = 0;
        private double vin;
    }
}
