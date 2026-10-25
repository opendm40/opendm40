//
// Data reads from the slow flash bank while the AD2 stream ISR owns the bus
// (2026-10-23, opendm40 TODO "Region C DATA reads during a stream"). On the
// GD32F427 flash from 0x08080000 up is not zero-wait: one CPU load from it
// stalls the bus ~21.8 us and the stream ISR's entry with it (device
// 246afc7: INFO/CALIB.BIN, LOADER.BIN, BOOTDATA.BIN and the FIRMWARE.BIN
// tail served by the USB volume while a fast-log session ran). emu/regionc.py
// hooks the region C functions (code fetches); this watches DATA loads:
// tlib's memory access hook (TranslationCPU.SetHookAtMemoryAccess), every
// CPU data read in [lo, hi) while the byte at `handed` (hy3131::HANDED) is
// non-zero is recorded as "pc addr" in `path` (appended, once per pc; the
// count per pc in Dump). Also "pc code": the stream ISR interrupted code
// running in [lo, hi) (sampled at each handler entry). Off until Arm (the hook makes every load and store
// call out of the translated code: slower).
//   sysbus.flashwatch Arm 0x08080000 0x08100000 0x1000xxxx "/path/hits"
//   sysbus.flashwatch Dump          (pc count first-addr, logged)
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Logging.Profiling;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.CPU;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class FlashWatch : IDoubleWordPeripheral, IKnownSize
    {
        public FlashWatch(IMachine machine)
        {
            this.machine = machine;
        }

        public void Reset()
        {
        }

        public long Size => 0x100;

        public uint ReadDoubleWord(long offset)
        {
            return 0;
        }

        public void WriteDoubleWord(long offset, uint value)
        {
        }

        public void Arm(ulong lo, ulong hi, ulong handed, string path)
        {
            this.lo = lo;
            this.hi = hi;
            this.handed = handed;
            this.path = path;
            foreach(var c in machine.SystemBus.GetCPUs())
            {
                if(c is TranslationCPU t)
                {
                    cpu = t;
                    break;
                }
            }
            if(cpu == null)
            {
                this.Log(LogLevel.Error, "no translation CPU");
                return;
            }
            cpu.SetHookAtMemoryAccess((pc, op, vaddr, paddr, width, value) =>
            {
                if(paddr < this.lo || paddr >= this.hi)
                {
                    return;
                }
                if(op != MemoryOperation.MemoryRead && op != MemoryOperation.MemoryIORead)
                {
                    return;
                }
                if(machine.SystemBus.ReadByte(this.handed) == 0)
                {
                    return;
                }
                hits.TryGetValue(pc, out var n);
                hits[pc] = n + 1;
                if(n == 0)
                {
                    firstAddr[pc] = paddr;
                    System.IO.File.AppendAllText(this.path, string.Format("{0:x} {1:x}\n", pc, paddr));
                }
            });
            // Code: the stream ISR found the main loop executing in the slow
            // bank (the interrupted PC): code entered before the stream
            // started and still running (a function's tail after
            // stream::start), which the per-function entry hooks of
            // regionc.py do not see. Sampled at every handler entry.
            cpu.AddHookAtInterruptBegin(_ =>
            {
                var pc = (ulong)cpu.PC;
                if(pc < this.lo || pc >= this.hi || machine.SystemBus.ReadByte(this.handed) == 0)
                {
                    return;
                }
                fetches.TryGetValue(pc, out var n);
                fetches[pc] = n + 1;
                if(n == 0)
                {
                    System.IO.File.AppendAllText(this.path, string.Format("{0:x} code\n", pc));
                }
            });
            this.Log(LogLevel.Info, "armed: data reads 0x{0:X8}..0x{1:X8} while [0x{2:X8}] != 0 -> {3}", lo, hi, handed, path);
        }

        public string Dump()
        {
            var r = new System.Text.StringBuilder();
            foreach(var kv in hits)
            {
                r.AppendFormat("{0:x8} {1} {2:x8}\n", kv.Key, kv.Value, firstAddr[kv.Key]);
            }
            this.Log(LogLevel.Info, "slow-bank reads while handed: {0}", r.Length == 0 ? "none" : r.ToString());
            return r.ToString();
        }

        private readonly IMachine machine;
        private TranslationCPU cpu;
        private ulong lo, hi, handed;
        private string path;
        private readonly System.Collections.Generic.Dictionary<ulong, ulong> hits = new System.Collections.Generic.Dictionary<ulong, ulong>();
        private readonly System.Collections.Generic.Dictionary<ulong, ulong> fetches = new System.Collections.Generic.Dictionary<ulong, ulong>();
        private readonly System.Collections.Generic.Dictionary<ulong, ulong> firstAddr = new System.Collections.Generic.Dictionary<ulong, ulong>();
    }
}
