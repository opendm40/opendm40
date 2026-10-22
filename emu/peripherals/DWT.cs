//
// Cortex-M DWT at 0xE0001000: CYCCNT derived from virtual time (HCLK 200 MHz),
// writable (fw-rs zeroes it at start); CTRL is plain storage with CYCCNTENA.
// Only CTRL/CYCCNT are modelled; the rest read 0.
//
using Antmicro.Renode.Core;
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
        }

        public void Reset()
        {
            ctrl = 0;
            offset = 0;
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
            case 0x4: offset = value - (uint)Cycles(); break;
            }
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
        private uint ctrl, offset;
    }
}
