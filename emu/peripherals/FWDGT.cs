//
// GD32F4 free watchdog (FWDGT = STM32 IWDG layout) at 0x40003000, plus the
// reset-cause flags of RCU_RSTSCK (0x40023874, bits 25..31) that survive a
// reset. Renode 1.17.0's STM32_IndependentWatchdog never reset the machine
// here, and its RCC returns a constant 0x0E000000 from RSTSCK (RSTFC
// ignored), so opendm40's shim (src/boot.rs) could never see a watchdog reset.
//
//   CTL  +0x00  key: 0xCCCC start, 0xAAAA reload, 0x5555 unlock PSC / RLD
//   PSC  +0x04  prescaler /4 << PSC (0..6), RLD +0x08 reload (12 bit),
//   STAT +0x0C  PUD / RUD: always 0 (updates take effect at once)
//   Clock: IRC32K, 32 kHz (stock PSC 6, RLD 2500 -> 20 s; measured ~19 s).
//   Timeout: the machine resets (machine.RequestReset, as SYSRESETREQ does:
//   the flash, the backup SRAM and all other memories keep their content;
//   run.py's `reset` macro restores the vector table base).
//
// Reset flags (RCU_RSTSCK 25 BOR, 26 PIN (EP), 27 POR, 28 SW, 29 FWDGT,
// 30 WWDGT, 31 LP): power-on = BOR | PIN | POR, as the real reset value
// 0x0E000000. They are sticky until RSTFC (bit 24) is written: a watchdog
// timeout adds FWDGT | PIN, a SYSRESETREQ adds SW | PIN (the reset pulse
// also drives NRST, as on the STM32F4). RSTSCK itself belongs to the RCC
// model, so run.py routes it here with two bus hooks:
//   sysbus.rcc read  0x74 -> (rcc value & 0x00FFFFFF) | Flags   (EMU_FLAGS)
//   sysbus.rcc write 0x74 with bit 24 -> Flags = 0               (EMU_FLAGS)
//   sysbus.nvic write 0xD0C (AIRCR) VECTKEY + SYSRESETREQ -> SW  (EMU_SWRST)
// through the emulator-only registers below (never used by firmware).
//
// Monitor: `sysbus.fwdgt Flags` (get / set), `sysbus.fwdgt Running`,
// `sysbus.fwdgt Timeouts` (count since start), `sysbus.fwdgt PowerOn` (the
// power-on flags again, for a power-cycle in a script).
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.Timers
{
    public class DM40_FWDGT : IDoubleWordPeripheral, IKnownSize
    {
        public DM40_FWDGT(IMachine machine, ulong frequency = 32000)
        {
            this.machine = machine;
            this.frequency = frequency;
            Flags = PowerOnFlags;
            Reset();
        }

        public const uint BOR = 1u << 25, PIN = 1u << 26, POR = 1u << 27, SW = 1u << 28, FWDGT = 1u << 29;
        public const uint PowerOnFlags = BOR | PIN | POR;
        /// Emulator-only registers (offsets inside this 1 KiB block).
        public const long EMU_FLAGS = 0x3F0, EMU_SWRST = 0x3F4;

        public long Size => 0x400;

        /// RCU_RSTSCK reset-cause bits (25..31), sticky across resets.
        public uint Flags { get; set; }
        public bool Running => running;
        public int Timeouts { get; private set; }

        public void PowerOn()
        {
            Flags = PowerOnFlags;
        }

        public void Reset()
        {
            // Called on every machine reset: record its cause, then the
            // watchdog is stopped (software watchdog option, nWDG_HW = 1).
            if(timedOut)
            {
                Flags |= FWDGT | PIN;
            }
            else if(swPending)
            {
                Flags |= SW | PIN;
            }
            timedOut = false;
            swPending = false;
            running = false;
            unlocked = false;
            psc = 0;
            rld = 0xFFF;
            scheduled = false;
            gen++;
        }

        public uint ReadDoubleWord(long offset)
        {
            switch(offset)
            {
            case 0x04: return psc;
            case 0x08: return rld;
            case 0x0C: return 0;
            case EMU_FLAGS: return Flags;
            default: return 0;
            }
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            switch(offset)
            {
            case 0x00:
                switch(value & 0xFFFF)
                {
                case 0xCCCC:
                    if(!running)
                    {
                        running = true;
                        this.Log(LogLevel.Info, "started (PSC {0}, RLD {1})", psc, rld);
                    }
                    Reload();
                    break;
                case 0xAAAA:
                    unlocked = false;
                    Reload();
                    break;
                case 0x5555:
                    unlocked = true;
                    break;
                default:
                    unlocked = false;
                    break;
                }
                break;
            case 0x04:
                if(unlocked) psc = value & 7;
                break;
            case 0x08:
                if(unlocked) rld = value & 0xFFF;
                break;
            case EMU_FLAGS:
                Flags = value & 0xFE000000;
                break;
            case EMU_SWRST:
                swPending = true;
                break;
            }
        }

        private double Now()
        {
            if(machine.SystemBus.TryGetCurrentCPU(out var cpu)) cpu.SyncTime();
            return machine.ElapsedVirtualTime.TimeElapsed.TotalSeconds;
        }

        private void Reload()
        {
            if(!running) return;
            var div = 4u << (int)Math.Min(psc, 6u);
            deadline = Now() + (rld + 1) * (double)div / frequency;
            if(!scheduled)
            {
                Schedule(gen);
            }
        }

        private void Schedule(ulong g)
        {
            scheduled = true;
            var dt = Math.Max(1e-6, deadline - Now());
            machine.ScheduleAction(TimeInterval.FromMicroseconds((ulong)Math.Ceiling(dt * 1e6)), _ => Check(g));
        }

        private void Check(ulong g)
        {
            if(g != gen) return; // a reset in between (Reset cleared `scheduled`)
            scheduled = false;
            if(!running) return;
            if(Now() + 1e-6 < deadline)
            {
                Schedule(g);
                return;
            }
            Timeouts++;
            this.Log(LogLevel.Info, "timeout at {0:F3} s: reset", Now());
            timedOut = true;
            machine.RequestReset();
        }

        private readonly IMachine machine;
        private readonly ulong frequency;
        private bool running, unlocked, timedOut, swPending, scheduled;
        private uint psc, rld;
        private double deadline;
        private ulong gen;
    }
}
