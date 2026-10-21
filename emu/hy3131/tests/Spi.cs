// A GPIO port stub and a bit-banging SPI master that talks to the real HY3131
// peripheral class exactly like the firmware (hy3131_v2_6.h frame).
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;
using Antmicro.Renode.Core;
using Antmicro.Renode.Peripherals.Miscellaneous;

namespace Hy3131Tests
{
    public class PortStub : INumberedGPIOOutput, IGPIOReceiver
    {
        public PortStub()
        {
            var c = new Dictionary<int, IGPIO>();
            for(var i = 0; i < 16; i++) c[i] = new GPIO();
            Connections = new ReadOnlyDictionary<int, IGPIO>(c);
        }
        public IReadOnlyDictionary<int, IGPIO> Connections { get; }
        public bool[] Level = new bool[16];
        /// External drive (the chip's DO into our MISO pin).
        public void OnGPIO(int number, bool value) { Level[number] = value; }
        /// MCU drive of an output pin.
        public void Drive(int pin, bool v) { Level[pin] = v; Connections[pin].Set(v); }
    }

    public class Master
    {
        public readonly MachineStub Machine = new MachineStub();
        public readonly PortStub PortC = new PortStub(), PortD = new PortStub(), PortA = new PortStub(), PortE = new PortStub();
        public readonly HY3131 Chip;
        public double BitTime = 1e-6; // seconds per SCK phase (the stock is faster, fw-rs ~1 us)

        public Master()
        {
            PortD.Drive(2, true);
            Chip = new HY3131(Machine, PortC, PortD, 10, 11, 12, 2, PortA, PortE);
        }

        /// The firmware's relay drive (dmm.rs relay_pa / relay_pe / pc7): PA latching pulse
        /// (0 = V/A path, 1 = ohm/mV network), PE shunt (0 A, 1 mA, 2 none), PC7.
        public void Relays(int pa, int pe, bool pc7 = false)
        {
            var (set, clr) = pa == 0 ? (10, 9) : (9, 10);
            PortA.Drive(clr, false); PortA.Drive(set, true); Advance(0.010); PortA.Drive(set, false);
            PortE.Drive(0, pe == 0); PortE.Drive(1, pe == 1);
            PortC.Drive(7, pc7);
        }

        /// Relay state of a stock mode (hy3131_v2_6.h "relays" column).
        public void RelaysForMode(int m)
        {
            var pa = (m == 0 || m == 1 || (m >= 4 && m <= 9)) ? 0 : 1;
            var pe = (m == 4 || m == 7) ? 0 : (m == 5 || m == 8) ? 1 : 2;
            Relays(pa, pe, m == 14);
        }

        public double Now { get => Machine.Now; set => Machine.Now = value; }
        public void Advance(double s) { Machine.Now += s; }

        private void Clock()
        {
            PortC.Drive(10, true); Advance(BitTime);
            PortC.Drive(10, false); Advance(BitTime);
        }

        private void Tx(byte b)
        {
            for(var i = 7; i >= 0; i--)
            {
                PortC.Drive(12, (b >> i & 1) != 0);
                Clock();
            }
        }

        private byte Rx()
        {
            byte b = 0;
            for(var i = 0; i < 8; i++)
            {
                b = (byte)((b << 1) | (PortC.Level[11] ? 1 : 0));
                Clock();
            }
            return b;
        }

        public void Write(int reg, params byte[] data)
        {
            PortC.Drive(10, false); PortD.Drive(2, false); Advance(BitTime);
            Tx((byte)(reg << 1));
            foreach(var b in data) Tx(b);
            Advance(BitTime); PortD.Drive(2, true); PortC.Drive(10, false);
        }

        public byte[] Read(int reg, int n)
        {
            PortC.Drive(10, false); PortD.Drive(2, false); Advance(BitTime);
            Tx((byte)((reg << 1) | 1));
            Clock(); // read period
            var r = new byte[n];
            for(var i = 0; i < n; i++) r[i] = Rx();
            Advance(BitTime); PortD.Drive(2, true); PortC.Drive(10, false);
            return r;
        }

        public byte Read1(int reg) => Read(reg, 1)[0];

        public int ReadS24(int reg)
        {
            var b = Read(reg, 3);
            return (int)((uint)(b[0] | b[1] << 8 | b[2] << 16) << 8) >> 8;
        }

        /// Stock mode change: zero R26/R34/R36, relays of stock mode `mode` (-1 = unchanged),
        /// the 22-byte row, INTE = 0.
        public void Configure(byte[] row, int mode = -1)
        {
            Write(0x26, 0); Write(0x34, 0); Write(0x36, 0);
            if(mode >= 0) RelaysForMode(mode);
            Write(0x20, row);
            Write(0x1F, 0);
        }

        /// Wait (virtual time, polling INTF every `poll` s) for the flag bits, return INTF.
        public byte WaitFlag(int mask, double timeout, double poll = 0.01)
        {
            var end = Now + timeout;
            while(Now < end)
            {
                var f = Read1(0x1E);
                if((f & mask) == mask) return f;
                Advance(poll);
            }
            return 0;
        }

        public static byte[] Row(string hex)
        {
            var p = hex.Split(' ');
            var r = new byte[p.Length];
            for(var i = 0; i < p.Length; i++) r[i] = Convert.ToByte(p[i], 16);
            return r;
        }
    }
}
