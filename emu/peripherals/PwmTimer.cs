//
// Event-free general/advanced timer for the DM40's output-only timers (TIM1/8
// scope sample clocks, TIM3 offset PWM, TIM6 generator trigger, TIM12 PWM,
// TIM13 backlight, TIM14 buzzer). Renode's STM32_Timer handles every counter
// overflow as an event: TIM8 at 50 MHz and TIM6 at 10 MHz then cost more wall
// time than the CPU. Nothing in the firmware takes interrupts from these
// timers, so this model only stores the registers; CNT is computed from
// virtual time while CEN is set (counting up, 200 MHz timer clock), UIF is set
// on read when the counter wrapped since the last clear, EGR.UG resets CNT.
// The scope feeder reads PSC/ARR/CR1/SMCR from here.
// LogWrites(path): every write to ARR / CCR1 that changes the register goes to
// `path` as "<virtual ms> <ARR|CCR1> <value>" (the buzzer tests: TIM14 duty =
// loudness, ARR = pitch; emu/hy3131/loudtest.py).
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Timers
{
    public class PwmTimer : IDoubleWordPeripheral, IWordPeripheral, IKnownSize
    {
        public PwmTimer(IMachine machine, ulong frequency = 200000000)
        {
            this.machine = machine;
            this.frequency = frequency;
            Reset();
        }

        public void LogWrites(string path)
        {
            log = new System.IO.StreamWriter(path, false) { AutoFlush = true };
        }

        public void Reset()
        {
            Array.Clear(regs, 0, regs.Length);
            regs[ARR / 4] = 0xFFFF;
            startTicks = 0; cntAtStart = 0; lastWraps = 0;
        }

        public long Size => 0x400;

        public ushort ReadWord(long offset) => (ushort)ReadDoubleWord(offset);
        public void WriteWord(long offset, ushort value) => WriteDoubleWord(offset, value);

        public uint ReadDoubleWord(long offset)
        {
            switch(offset)
            {
            case CNT: return (uint)Counter(out _);
            case SR:
                {
                    Counter(out var wraps);
                    if(wraps != lastWraps)
                    {
                        regs[SR / 4] |= 1; // UIF
                        lastWraps = wraps;
                    }
                    return regs[SR / 4];
                }
            default: return offset < Size ? regs[offset / 4] : 0;
            }
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            switch(offset)
            {
            case CR1:
                {
                    var running = (regs[CR1 / 4] & 1) != 0;
                    var cnt = Counter(out _);
                    regs[CR1 / 4] = value;
                    if((value & 1) != 0 && !running) Start(cnt);
                    else if((value & 1) == 0 && running) cntAtStart = cnt;
                }
                break;
            case CNT:
                regs[CNT / 4] = value;
                Start(value);
                break;
            case SR: regs[SR / 4] &= value; break; // rc_w0
            case EGR:
                if((value & 1) != 0) Start(0); // UG
                break;
            default:
                if(log != null && (offset == ARR || offset == CCR1) && regs[offset / 4] != value)
                {
                    log.WriteLine("{0:F3} {1} {2}", machine.ElapsedVirtualTime.TimeElapsed.TotalMilliseconds,
                                  offset == ARR ? "ARR" : "CCR1", value);
                }
                if(offset < Size) regs[offset / 4] = value;
                break;
            }
        }

        private void Start(ulong cnt)
        {
            cntAtStart = cnt;
            startTicks = Ticks();
            lastWraps = 0;
        }

        private ulong Ticks()
        {
            return (ulong)(machine.ElapsedVirtualTime.TimeElapsed.TotalSeconds * frequency);
        }

        private ulong Counter(out ulong wraps)
        {
            wraps = 0;
            if((regs[CR1 / 4] & 1) == 0) return cntAtStart;
            var psc = (regs[PSC / 4] & 0xFFFF) + 1UL;
            var period = (ulong)regs[ARR / 4] + 1UL;
            var counts = (Ticks() - startTicks) / psc + cntAtStart;
            wraps = counts / period;
            return counts % period;
        }

        private const long CR1 = 0x00, SR = 0x10, EGR = 0x14, CNT = 0x24, PSC = 0x28, ARR = 0x2C, CCR1 = 0x34;
        private System.IO.StreamWriter log;
        private readonly IMachine machine;
        private readonly ulong frequency;
        private readonly uint[] regs = new uint[0x100];
        private ulong startTicks, cntAtStart, lastWraps;
    }
}
