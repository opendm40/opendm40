//
// GD32F4 SPI (master, 8-bit, the parts fw-rs uses) with DMA requests and the
// byte timing of the real peripheral (2026-10-22, fw-rs TODO "76.8 kS/s": the
// AD2 stream's frames must fit a 13 us period, so the model may not make
// them instantaneous):
//   * a byte takes 8 SCK periods, SCK = PCLK1 (ApbHz, 50 MHz) / 2^(PSC + 1)
//     (CTL0 bits 5:3: /8 = 1.28 us, /16 = 2.56 us per byte);
//   * a transmit buffer and a shift register: a DATA write with the shifter
//     idle starts shifting at once (TBE stays 1), one more byte waits in the
//     buffer (TBE 0) and follows back to back; RBNE at the end of each byte;
//     STAT TRANS while a byte shifts or waits;
//   * DMA (GD32_DMA, request = stream number): TBE with DMATEN requests the TX
//     stream (a level: again after every byte moved to the shifter, and when
//     the stream or DMATEN is switched on), RBNE with DMAREN the RX stream;
//   * lazy: nothing runs per byte in virtual time. Every access that can
//     observe the bus (SPI registers, the DMA streams' registers through
//     GD32_DMA.BeforeStreamRead, the slave's own CS / SCK pin events through
//     ITimedSpiPeripheral.SetCatchUp) first completes the bytes whose end
//     time has passed, in order, each handed to the slave with its start
//     time, SCK period and CKPH (HY3131SpiPort -> HY3131.SpiByteTimed: the
//     chip's state at each clock edge, the mode-0 DO margin at /8, CKPH 1
//     sampling). A byte still shifting when CS goes high reaches the chip
//     after CS high, i.e. not at all (fw-rs's cut dummy byte of a write).
// Timed = false gives the old instantaneous bytes (a byte completes at the
// next DATA write / STAT read / DATA read).
// Replaces Renode's STM32SPI for SPI2 (dm40.repl spi3), which has no DMA
// requests: fw-rs's AD2 stream DMA frames (hy3131.rs `stream::dma`).
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.CPU;
using Antmicro.Renode.Peripherals.DMA;

namespace Antmicro.Renode.Peripherals.SPI
{
    public class GD32_SPI : NullRegistrationPointPeripheralContainer<ISPIPeripheral>, IDoubleWordPeripheral, IWordPeripheral, IBytePeripheral, IKnownSize
    {
        public GD32_SPI(IMachine machine, GD32_DMA dma = null, int rxStream = 0, int txStream = 7) : base(machine)
        {
            this.machine = machine;
            this.dma = dma;
            this.rxStream = rxStream;
            this.txStream = txStream;
            if(dma != null)
            {
                dma.StreamEnabled += OnStreamEnabled;
                dma.BeforeStreamRead += s => { if(s == rxStream || s == txStream) CatchUp(); };
            }
            Reset();
        }

        public override void Reset()
        {
            ctl0 = 0; ctl1 = 0; rx = 0; rbne = false;
            shifting = false; bufFull = false;
        }

        public long Size => 0x400;

        /// PCLK1, Hz (the SCK divider's input).
        public double ApbHz { get; set; } = 50e6;
        /// Bytes take their SCK time (false: instantaneous, the old model).
        public bool Timed { get; set; } = true;

        public uint ReadDoubleWord(long offset)
        {
            switch(offset)
            {
            case 0x00: return ctl0;
            case 0x04: return ctl1;
            case 0x08:
                CatchUp();
                if(!Timed && !rbne) FinishNow();
                return (bufFull ? 0u : 0x02u) | (rbne ? 1u : 0u) | (shifting || bufFull ? 0x80u : 0u);
            case 0x0C:
                CatchUp();
                if(!Timed && !rbne) FinishNow();
                rbne = false;
                return rx;
            default: return 0;
            }
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            switch(offset)
            {
            case 0x00:
                CatchUp();
                if((value & 0x40) == 0 && (shifting || bufFull))
                {
                    // SPIEN off mid-byte: the byte is abandoned (fw-rs switches
                    // CKPH / the divider only when idle).
                    Aborted++;
                    shifting = false; bufFull = false;
                }
                ctl0 = value & 0xFFFF;
                break;
            case 0x04:
                CatchUp();
                ctl1 = value & 0xFF;
                if((ctl1 & 1) != 0 && rbne) RequestRx();
                PumpTx();
                break;
            case 0x0C:
                if((ctl0 & 0x40) == 0) break; // SPIEN
                CatchUp();
                if(!Timed && shifting) FinishNow();
                Load((byte)value);
                break;
            }
        }

        public ushort ReadWord(long offset) => (ushort)ReadDoubleWord(offset);
        public void WriteWord(long offset, ushort value) => WriteDoubleWord(offset, value);
        public byte ReadByte(long offset) => (byte)ReadDoubleWord(offset);
        public void WriteByte(long offset, byte value) => WriteDoubleWord(offset, value);

        /// Bytes exchanged; bytes written while the buffer was full (lost);
        /// bytes abandoned by SPIEN off (diagnostics).
        public ulong Bytes { get; private set; }
        public ulong Overruns { get; private set; }
        public ulong Aborted { get; private set; }

        /// The machine's time at this access: the CPU's progress inside its
        /// quantum is reported first (ICPU.SyncTime; without it the time stands
        /// still for a whole quantum and a polled byte never completes).
        private double MachineNow
        {
            get
            {
                if(machine.SystemBus.TryGetCurrentCPU(out var cpu)) cpu.SyncTime();
                return machine.ElapsedVirtualTime.TimeElapsed.TotalSeconds;
            }
        }
        private double TNow => catchingUp ? logical : MachineNow;
        private double BitTime => (2 << (int)((ctl0 >> 3) & 7)) / ApbHz;

        private void Load(byte b)
        {
            if(!catchUpSet && RegisteredPeripheral is ITimedSpiPeripheral ts)
            {
                ts.SetCatchUp(CatchUp);
                catchUpSet = true;
            }
            if(!shifting)
            {
                Start(b, TNow);
            }
            else if(!bufFull)
            {
                bufFull = true;
                bufTx = b;
            }
            else
            {
                Overruns++;
                bufTx = b;
            }
            PumpTx();
        }

        private void Start(byte b, double t)
        {
            shifting = true;
            shTx = b;
            shStart = t;
            shBit = Timed ? BitTime : 0;
            shCkph = (ctl0 & 1) != 0;
            shEnd = t + 8 * shBit;
        }

        /// Complete every byte whose end time has passed (in order, each at
        /// its own time: DMA requests, the next byte from the buffer).
        public void CatchUp()
        {
            if(catchingUp || !Timed) return;
            var now = MachineNow;
            catchingUp = true;
            try
            {
                while(shifting && shEnd <= now)
                {
                    logical = shEnd;
                    Complete();
                }
            }
            finally
            {
                catchingUp = false;
            }
        }

        /// Untimed model: the byte in flight completes now.
        private void FinishNow()
        {
            if(!shifting) return;
            catchingUp = true;
            logical = MachineNow;
            try { Complete(); } finally { catchingUp = false; }
        }

        private void Complete()
        {
            shifting = false;
            var p = RegisteredPeripheral;
            var ts = p as ITimedSpiPeripheral;
            if(p == null) rx = 0xFF;
            else if(ts != null && Timed) rx = ts.TransmitTimed(shTx, shStart, shBit, shCkph);
            else rx = p.Transmit(shTx);
            rbne = true;
            Bytes++;
            if(bufFull)
            {
                bufFull = false;
                Start(bufTx, shEnd);
            }
            if((ctl1 & 1) != 0) RequestRx();
            PumpTx();
        }

        private void RequestRx()
        {
            if(dma != null && dma.Enabled(rxStream)) dma.Kick(rxStream);
        }

        /// TBE (buffer empty) with DMATEN and the TX stream enabled: one request
        /// per byte. Not re-entered from the DMA's own DATA write.
        private void PumpTx()
        {
            if(pumping) { again = true; return; }
            pumping = true;
            do
            {
                again = false;
                if(!bufFull && (ctl1 & 2) != 0 && dma != null && dma.Enabled(txStream) && dma.Remaining(txStream) > 0)
                {
                    dma.Kick(txStream);
                }
            }
            while(again);
            pumping = false;
        }

        private void OnStreamEnabled(int stream)
        {
            if(stream == txStream) PumpTx();
            else if(stream == rxStream && (ctl1 & 1) != 0 && rbne) RequestRx();
        }

        private readonly IMachine machine;
        private readonly GD32_DMA dma;
        private readonly int rxStream, txStream;
        private uint ctl0, ctl1;
        private byte rx, shTx, bufTx;
        private bool rbne, pumping, again, shifting, bufFull, shCkph, catchingUp, catchUpSet;
        private double shStart, shEnd, shBit, logical;
    }
}
