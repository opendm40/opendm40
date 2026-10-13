//
// 3PA1030 scope ADC path (firmware/re/analysis/scope_gen_v2_6.h, fw-rs/src/scope.rs).
// The real chain is TIM1_CH4 -> DMA2 stream 4 reading GPIOB->IDR (PB0..PB9,
// 10-bit straight binary) at fs = 200 MHz / ((TIM1 PSC+1)(ARR+1)); below
// 50 MS/s stream 3 (TIM8_CH2) latches IDR into a word that stream 4 copies.
// Renode's timers raise no DMA requests, so this model is the "ADC + DMA
// request": while DMA2 stream 4 is enabled and TIM1 is counting it delivers
// samples to the stream at fs (in bulk every tick), computing each sample
// from a scripted waveform at virtual time: code = 512 + (v - offset) *
// counts_per_volt(range) where the range follows the front-end GPIOs
// (PC13/PB15/PD12/PD13 table of the stock).
// It also drives the trigger comparator output PA15 (TIM2_CH1): PA15 = sample
// code > level, level = DAC2 code mapped with the stock default calibration
// (DAC 1780 -> code 300, 2401 -> code 780).
// SetWave(shape, hz, vpp, offset): sine | square | triangle | ramp | dc | noise.
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.DMA;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class ScopeFeeder : IDoubleWordPeripheral, IKnownSize
    {
        public ScopeFeeder(IMachine machine, GD32_DMA dma, IGPIOReceiver portA, int stream = 4, ulong tickUs = 200)
        {
            this.machine = machine;
            this.dma = dma;
            this.portA = portA;
            this.stream = stream;
            this.tickUs = tickUs;
            SetWave("sine", 1000, 1.0, 0.0);
            Reset();
            // Self-rescheduling action on the emulation thread (a ClockEntry handler
            // tripped a time-framework semaphore when a tick fed many samples).
            machine.ScheduleAction(TimeInterval.FromMicroseconds(tickUs), _ => Tick());
        }

        public void Reset()
        {
            phase = 0;
            lastPa15 = false;
            Samples = 0;
        }

        public long Size => 0x100;
        public uint ReadDoubleWord(long offset) => offset == 0 ? (uint)LastCode : 0;
        public void WriteDoubleWord(long offset, uint value) { }

        public void SetWave(string shape, double hz, double vpp, double offset)
        {
            this.shape = shape.ToLowerInvariant();
            this.hz = hz;
            this.vpp = vpp;
            this.offset = offset;
            this.Log(LogLevel.Info, "wave {0} {1} Hz {2} Vpp offset {3} V", shape, hz, vpp, offset);
        }

        /// Trigger comparator on/off (PA15 driving).
        public bool Comparator { get; set; } = true;
        public ulong Samples { get; private set; }
        public int LastCode { get; private set; }
        public double Fs { get; private set; }
        public int Range { get; private set; }

        private void Tick()
        {
            try
            {
                Step();
            }
            catch(Exception e)
            {
                this.Log(LogLevel.Error, "feeder: {0}", e.Message);
            }
            machine.ScheduleAction(TimeInterval.FromMicroseconds(tickUs), _ => Tick());
        }

        private void Step()
        {
            // TIM1 running and stream enabled?
            var bus = machine.SystemBus;
            if(!dma.Enabled(stream))
            {
                return;
            }
            var tim1Cr1 = bus.ReadDoubleWord(TIM1);
            var smcr = bus.ReadDoubleWord(TIM1 + 0x08);
            if((tim1Cr1 & 1) == 0)
            {
                // Stock: TIM1 is started by TIM5's TRGO (slave trigger mode, TS = ITR0),
                // which Renode's timer does not model: treat a running TIM5 as the start.
                var slaveTrigger = (smcr & 7) == 6 && ((smcr >> 4) & 7) == 0;
                if(!(slaveTrigger && (bus.ReadDoubleWord(TIM5) & 1) != 0))
                {
                    return;
                }
            }
            // TIM1 gated by TIM2 (fw-rs hardware trigger): SMCR SMS=101 and TIM2 OC2REF low -> stopped.
            if((smcr & 7) == 5)
            {
                // Gate = TIM2 OC2REF: high while TIM2 CNT < CCR2 or TIM2 not running.
                var t2cr1 = bus.ReadDoubleWord(TIM2);
                if((t2cr1 & 1) != 0)
                {
                    var cnt = bus.ReadDoubleWord(TIM2 + 0x24);
                    var ccr2 = bus.ReadDoubleWord(TIM2 + 0x38);
                    if(cnt >= ccr2) return;
                }
            }
            var psc = bus.ReadDoubleWord(TIM1 + 0x28) & 0xFFFF;
            var arr = bus.ReadDoubleWord(TIM1 + 0x2C) & 0xFFFF;
            Fs = 200e6 / ((psc + 1.0) * (arr + 1.0));
            var now = machine.ElapsedVirtualTime.TimeElapsed.TotalSeconds;
            // A new acquisition (stream re-enabled, timers restarted): the start was
            // seen up to one tick late, so samples are fed one tick AHEAD of virtual
            // time (their values come from the waveform time, so this is harmless) and
            // the count follows elapsed time instead of a fixed batch size.
            var gen = dma.EnableCount(stream);
            if(gen != lastGen || Fs != lastFs)
            {
                if(Fs != lastFs)
                {
                    this.Log(LogLevel.Info, "acquisition at {0} S/s, range {1}", Fs, Range);
                }
                lastGen = gen; lastFs = Fs;
                t0 = now; fed = 0;
            }
            var due = (long)Math.Floor((now + tickUs / 1e6 - t0) * Fs) - fed;
            var n = (int)Math.Min(due, 200000);
            if(n <= 0) return;
            UpdateRange(bus);
            var level = TriggerCode(bus);
            var dt = 1.0 / Fs;
            var t = t0 + fed * dt;
            if(block.Length < n) block = new ushort[n];
            var start = 0;
            for(var i = 0; i < n; i++)
            {
                var v = Volts(t + i * dt);
                var code = (int)Math.Round(512.0 + v * countsPerVolt);
                if(code < 0) code = 0;
                if(code > 1023) code = 1023;
                block[i] = (ushort)code;
                if(Comparator)
                {
                    var pa15 = code > level;
                    if(pa15 != lastPa15)
                    {
                        // Deliver the samples up to the edge first, so the DMA position
                        // seen by the trigger logic matches the edge.
                        var took = dma.FeedBlock(stream, block, start, i + 1 - start);
                        fed += took; Samples += (ulong)took;
                        start = i + 1;
                        lastPa15 = pa15;
                        portA.OnGPIO(15, pa15);
                        if(!dma.Enabled(stream)) { LastCode = code; return; }
                    }
                }
                LastCode = code;
            }
            if(start < n)
            {
                var took = dma.FeedBlock(stream, block, start, n - start);
                fed += took; Samples += (ulong)took;
            }
        }

        private double Volts(double t)
        {
            var ph = (t * hz) % 1.0;
            double x;
            switch(shape)
            {
            case "square": x = ph < 0.5 ? 1 : -1; break;
            case "triangle": x = ph < 0.5 ? 4 * ph - 1 : 3 - 4 * ph; break;
            case "ramp": x = 2 * ph - 1; break;
            case "dc": x = 0; break;
            case "noise": x = 2 * rng.NextDouble() - 1; break;
            default: x = Math.Sin(2 * Math.PI * ph); break;
            }
            return offset + x * vpp / 2;
        }

        // Front-end range from PC13/PB15/PD12/PD13 (stock scope_range_gpio table).
        private void UpdateRange(IBusController bus)
        {
            var pc13 = (bus.ReadDoubleWord(GPIOC + 0x14) >> 13) & 1;
            var pb15 = (bus.ReadDoubleWord(GPIOB + 0x14) >> 15) & 1;
            var pd12 = (bus.ReadDoubleWord(GPIOD + 0x14) >> 12) & 1;
            var pd13 = (bus.ReadDoubleWord(GPIOD + 0x14) >> 13) & 1;
            var r = 10;
            for(var i = 0; i < RangeGpio.Length; i++)
            {
                if(RangeGpio[i][0] == pc13 && RangeGpio[i][1] == pb15 && RangeGpio[i][2] == pd12 && RangeGpio[i][3] == pd13)
                {
                    r = i;
                    break;
                }
            }
            Range = r;
            countsPerVolt = CountsPerVolt[r];
        }

        private int TriggerCode(IBusController bus)
        {
            var dac = (int)(bus.ReadDoubleWord(DAC_DHR12R2) & 0xFFF);
            return 300 + (int)((dac - 1780) * (780.0 - 300.0) / (2401.0 - 1780.0));
        }

        private static readonly uint[][] RangeGpio =
        {
            new uint[] { 1, 1, 1, 0 }, new uint[] { 1, 1, 1, 1 }, new uint[] { 1, 0, 1, 1 }, new uint[] { 1, 0, 0, 1 },
            new uint[] { 1, 0, 1, 0 }, new uint[] { 0, 1, 1, 0 }, new uint[] { 0, 1, 1, 1 }, new uint[] { 0, 1, 0, 1 },
            new uint[] { 0, 0, 0, 1 }, new uint[] { 0, 0, 1, 0 }, new uint[] { 0, 0, 0, 0 },
        };
        private static readonly double[] CountsPerVolt =
            { 10800, 5375, 2150, 1080, 537.5, 215, 108, 53.75, 21.5, 11, 5.5 };

        private const ulong TIM1 = 0x40010000;
        private const ulong TIM2 = 0x40000000;
        private const ulong TIM5 = 0x40000C00;
        private const ulong GPIOB = 0x40020400;
        private const ulong GPIOC = 0x40020800;
        private const ulong GPIOD = 0x40020C00;
        private const ulong DAC_DHR12R2 = 0x40007414;

        private readonly IMachine machine;
        private readonly GD32_DMA dma;
        private readonly IGPIOReceiver portA;
        private readonly int stream;
        private readonly ulong tickUs;
        private readonly Random rng = new Random(1);
        private string shape;
        private double hz, vpp, offset, phase, countsPerVolt = 108;
        private bool lastPa15;
        private ushort[] block = new ushort[4096];
        private ulong lastGen;
        private double lastFs, t0;
        private long fed;
    }
}
