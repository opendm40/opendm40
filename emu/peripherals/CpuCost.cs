//
// CPU cost model (2026-10-23, fw-rs TODO "76.8 kS/s"). The emulator runs one
// instruction per 200 MHz cycle with no flash / APB / bus wait states; the
// device is ~1.8x slower per instruction (an idle AC V main-loop pass: 23 us
// on the device, 12.7 us here, 22-23 us at `run.py --mips 110`). That
// factor is the CPU's PerformanceInMips (run.py --mips, fastlogtest.py
// --mips). On top of it `IsrStallNs n` stalls every interrupt handler (fw-rs
// enables one: TIMER6, the stream ISR) for n ns of virtual time, right after
// the handler's first DWT CYCCNT read (DWT.cs calls OnCycRead; Renode
// realizes a SkipTime at the next time sync, the handler's last CYCCNT read):
// inside the busy window fw-rs measures (logger.rs BUSY16, DBG:ISR?
// cpu_permille), after the sample's stamp. 0 = off (the default).
// It stands for ISR work the model does not produce (device 7409b92 at
// 76 800 S/s: 1.48 handlers per sample against 1.13 here, cpu_permille 544
// against 461 at --mips 110); it lengthens every handler, so a large n
// also makes overruns the device does not have.
//   sysbus.cpucost IsrStallNs 700
//   sysbus.cpucost Stats        (handlers, stall us; logged)
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.CPU;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class CpuCost : IDoubleWordPeripheral, IKnownSize
    {
        public CpuCost(IMachine machine)
        {
            this.machine = machine;
            Instance = this;
        }

        // The model of the machine (one machine per Renode process here).
        public static CpuCost Instance;

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

        public uint IsrStallNs
        {
            get => isrStallNs;
            set
            {
                isrStallNs = value;
                Install();
            }
        }

        // DWT.cs, on every CYCCNT read.
        public void OnCycRead()
        {
            if(depth == 0 || ++readsThis != 1 || isrStallNs == 0)
            {
                return;
            }
            // Whole microseconds, the rest carried over.
            debtNs += isrStallNs;
            if(debtNs >= 1000)
            {
                var us = debtNs / 1000;
                debtNs -= us * 1000;
                stallUs += us;
                cpu.SkipTime(TimeInterval.FromMicroseconds(us));
            }
        }

        public string Stats()
        {
            Install();
            var r = string.Format("handlers {0} stall_ns {1} stalled_us {2} mips {3}", handlers, isrStallNs, stallUs, cpu?.PerformanceInMips);
            this.Log(LogLevel.Info, r);
            return r;
        }

        // Debug: the interrupted PC of every handler entry (a sampler of the
        // main code, ~1 per sample period while the stream runs) -> `path`
        // ("pc count" lines) at PcSampleDump.
        public void PcSampleStart()
        {
            Install();
            pcs = new System.Collections.Generic.Dictionary<ulong, ulong>();
        }

        public void PcSampleDump(string path)
        {
            var lines = new System.Collections.Generic.List<string>();
            if(pcs != null)
            {
                foreach(var kv in pcs)
                {
                    lines.Add(string.Format("{0:x8} {1}", kv.Key, kv.Value));
                }
            }
            System.IO.File.WriteAllLines(path, lines);
            pcs = null;
        }

        private void Install()
        {
            if(cpu != null)
            {
                return;
            }
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
            cpu.AddHookAtInterruptBegin(_ =>
            {
                if(depth++ == 0 && pcs != null)
                {
                    var pc = (ulong)cpu.PC;
                    pcs.TryGetValue(pc, out var n);
                    pcs[pc] = n + 1;
                }
            });
            cpu.AddHookAtInterruptEnd(_ =>
            {
                if(depth > 0 && --depth == 0)
                {
                    handlers++;
                    readsThis = 0;
                }
            });
        }

        private readonly IMachine machine;
        private TranslationCPU cpu;
        private int depth, readsThis;
        private uint isrStallNs;
        private ulong debtNs, stallUs, handlers;
        private System.Collections.Generic.Dictionary<ulong, ulong> pcs;
    }
}
