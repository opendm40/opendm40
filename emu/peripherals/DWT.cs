//
// Cortex-M DWT at 0xE0001000: CYCCNT derived from virtual time (HCLK 200 MHz),
// writable (fw-rs zeroes it at start); CTRL is plain storage with CYCCNTENA.
// Only CTRL/CYCCNT are modelled; the rest read 0.
//
// Harness sync (emu/README.md "Input markers"): fw-rs's uptime is the
// virtual time since its last CYCCNT write (clock.rs Clock::new in main), so
// `FwUptime` = virtual time - `ZeroedAt` is the firmware's DBG:PING? uptime
// now, without the 21 s CYCCNT wrap. HY3131 SetInput logs it with every
// input change ("input: ... fw <uptime>"); tests key their check windows on
// that marker instead of a guessed boot offset.
//   sysbus.dwt ZeroedAt   (virtual s of the last CYCCNT write, -1 = none)
//   sysbus.dwt FwUptime   (s, -1 before the first write)
//
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.CPU;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class DM40_DWT : IDoubleWordPeripheral, IKnownSize
    {
        public DM40_DWT(IMachine machine, ulong frequency = 200000000)
        {
            this.machine = machine;
            this.frequency = frequency;
            Instance = this;
        }

        /// The machine's DWT (one machine per Renode process here).
        public static DM40_DWT Instance;

        /// Virtual time (s) of the firmware's last CYCCNT write; -1 = none yet.
        public double ZeroedAt { get; private set; } = -1;

        /// The firmware's uptime (s) as fw-rs's Clock counts it (CYCCNT since
        /// its last write, unwrapped); -1 before the first write.
        public double FwUptime => ZeroedAt < 0 ? -1 : Seconds() - ZeroedAt + (double)written / frequency;

        public void Reset()
        {
            ctrl = 0;
            offset = 0;
            ZeroedAt = -1;
        }

        public long Size => 0x1000;

        public uint ReadDoubleWord(long off)
        {
            switch(off)
            {
            case 0x0: return ctrl | 0x40000000; // NOCYCCNT = 0 (bit 25), NUMCOMP etc. 0
            case 0x4:
                var v = (uint)(Cycles() + offset);
                // CpuCost.cs: a handler's modelled extra cost after its
                // first read (realized by the next time sync: inside the
                // busy window fw-rs measures, logger.rs BUSY16).
                CpuCost.Instance?.OnCycRead();
                return v;
            default: return 0;
            }
        }

        public void WriteDoubleWord(long off, uint value)
        {
            switch(off)
            {
            case 0x0: ctrl = value & 1; break;
            case 0x4:
                offset = value - (uint)Cycles();
                ZeroedAt = Seconds();
                written = value;
                // The harness's clock marker (emu/emusync.py): fw uptime =
                // virtual time - this (+ value / frequency).
                this.Log(LogLevel.Info, "CYCCNT write 0x{0:X8} at {1}", value,
                    ZeroedAt.ToString("F6", System.Globalization.CultureInfo.InvariantCulture));
                break;
            }
        }

        private double Seconds()
        {
            if(machine.SystemBus.TryGetCurrentCPU(out var cpu)) cpu.SyncTime();
            return machine.ElapsedVirtualTime.TimeElapsed.TotalSeconds;
        }

        private ulong Cycles()
        {
            // The CPU's progress inside its quantum first (else CYCCNT stands
            // still for a whole quantum: stamps and the ISR's busy time).
            if(machine.SystemBus.TryGetCurrentCPU(out var cpu)) cpu.SyncTime();
            var t = machine.ElapsedVirtualTime.TimeElapsed;
            return (ulong)(t.TotalSeconds * frequency);
        }

        private readonly IMachine machine;
        private readonly ulong frequency;
        private uint ctrl, offset, written;
    }
}
