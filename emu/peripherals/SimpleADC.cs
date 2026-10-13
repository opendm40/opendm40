//
// Minimal STM32F4/GD32F4 ADC: software-started single conversions of the
// channel in SQR3[4:0]; EOC always set, DR = the configured raw value of that
// channel. Default raw 2730 on every channel = 4.0 V battery via the DM40's
// "raw * 3.0 / 4096 * 2". SetChannel(ch, raw) / SetVolts(ch, v) to change.
// Common registers (ADC_SYNCCTL at +0x304 on GD32) are plain storage.
//
using System;
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Analog
{
    public class SimpleADC : IDoubleWordPeripheral, IKnownSize
    {
        public SimpleADC(IMachine machine)
        {
            Reset();
        }

        public void Reset()
        {
            Array.Clear(regs, 0, regs.Length);
        }

        public long Size => 0x400;

        public uint ReadDoubleWord(long offset)
        {
            switch(offset)
            {
            case 0x00: // SR: EOC(1) set, STRT(4)
                return regs[0] | 0x12;
            case 0x4C: // DR
                {
                    var ch = (int)(regs[0x34 / 4] & 0x1F);
                    return Raw(ch);
                }
            default:
                return regs[offset / 4];
            }
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            switch(offset)
            {
            case 0x00:
                regs[0] = value & ~0x12u;
                break;
            case 0x08: // CR2/CTL1: SWSTART(30), RSTCLB(3) and CLB(2) self-clear (GD32 calibration)
                regs[2] = value & ~((1u << 30) | (1u << 3) | (1u << 2));
                break;
            default:
                regs[offset / 4] = value;
                break;
            }
        }

        public void SetChannel(int ch, uint raw)
        {
            values[ch] = raw & 0xFFF;
        }

        /// Pin voltage with a 3.0 V reference.
        public void SetVolts(int ch, double v)
        {
            var raw = (int)Math.Round(v / 3.0 * 4096.0);
            SetChannel(ch, (uint)Math.Max(0, Math.Min(4095, raw)));
        }

        public uint Default { get; set; } = 2730;

        private uint Raw(int ch)
        {
            uint v;
            return values.TryGetValue(ch, out v) ? v : Default;
        }

        private readonly uint[] regs = new uint[0x100];
        private readonly Dictionary<int, uint> values = new Dictionary<int, uint>();
    }
}
