//
// GD32F4 SPI (master, 8-bit, the parts fw-rs uses) with DMA requests:
// CTL0 (SPIEN, CKPH, PSC ignored: bytes are instantaneous), CTL1 DMAREN /
// DMATEN, STAT (TBE always 1, RBNE, TRANS 0), DATA. A DATA write with SPIEN
// exchanges one byte with the registered ISPIPeripheral (the HY3131 port on
// SPI2) and sets RBNE. DMA (GD32_DMA, request = stream number): RBNE with
// DMAREN requests the RX stream; TBE with DMATEN requests the TX stream while
// it is enabled (a level: requested again after every byte, and when the
// stream or DMATEN is switched on). Replaces Renode's STM32SPI for SPI2
// (dm40.repl spi3), which has no DMA requests: fw-rs's AD2 stream DMA frames
// (hy3131.rs `stream::dma`). Timing is not modelled, but a byte written to
// DATA is "in flight" (shifting) until the next DATA write, a STAT read with
// RBNE clear or a DATA read: it reaches the chip only then, with the CS level
// of that moment. That models fw-rs's cut dummy byte (hy3131 `write_spi`):
// CS goes high while the dummy shifts, and the chip never sees it.
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.DMA;

namespace Antmicro.Renode.Peripherals.SPI
{
    public class GD32_SPI : NullRegistrationPointPeripheralContainer<ISPIPeripheral>, IDoubleWordPeripheral, IWordPeripheral, IBytePeripheral, IKnownSize
    {
        public GD32_SPI(IMachine machine, GD32_DMA dma = null, int rxStream = 0, int txStream = 7) : base(machine)
        {
            this.dma = dma;
            this.rxStream = rxStream;
            this.txStream = txStream;
            if(dma != null) dma.StreamEnabled += OnStreamEnabled;
            Reset();
        }

        public override void Reset()
        {
            ctl0 = 0; ctl1 = 0; rx = 0; rbne = false; flight = false;
        }

        public long Size => 0x400;

        public uint ReadDoubleWord(long offset)
        {
            switch(offset)
            {
            case 0x00: return ctl0;
            case 0x04: return ctl1;
            case 0x08:
                if(!rbne) Complete();
                return 0x02u | (rbne ? 1u : 0u); // TBE; TRANS 0
            case 0x0C:
                if(!rbne) Complete();
                rbne = false;
                return rx;
            default: return 0;
            }
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            switch(offset)
            {
            case 0x00: ctl0 = value & 0xFFFF; break;
            case 0x04:
                ctl1 = value & 0xFF;
                if((ctl1 & 1) != 0 && rbne) RequestRx();
                PumpTx();
                break;
            case 0x0C:
                if((ctl0 & 0x40) == 0) break; // SPIEN
                Complete();
                flight = true;
                txByte = (byte)value;
                if(pumping) Complete(); // a DMA frame: back to back
                break;
            }
        }

        public ushort ReadWord(long offset) => (ushort)ReadDoubleWord(offset);
        public void WriteWord(long offset, ushort value) => WriteDoubleWord(offset, value);
        public byte ReadByte(long offset) => (byte)ReadDoubleWord(offset);
        public void WriteByte(long offset, byte value) => WriteDoubleWord(offset, value);

        /// Bytes exchanged (diagnostics).
        public ulong Bytes { get; private set; }

        /// The byte in flight reaches the chip (with CS as it is now).
        private void Complete()
        {
            if(!flight) return;
            flight = false;
            rx = RegisteredPeripheral != null ? RegisteredPeripheral.Transmit(txByte) : (byte)0xFF;
            rbne = true;
            Bytes++;
            if((ctl1 & 1) != 0) RequestRx();
            PumpTx();
        }

        private void RequestRx()
        {
            if(dma != null && dma.Enabled(rxStream)) dma.Kick(rxStream);
        }

        /// TBE is always set: while DMATEN and the TX stream is enabled, one
        /// request per byte. Not re-entered from the DMA's own DATA write.
        private void PumpTx()
        {
            if(pumping) { again = true; return; }
            pumping = true;
            do
            {
                again = false;
                if((ctl1 & 2) != 0 && dma != null && dma.Enabled(txStream) && dma.Remaining(txStream) > 0)
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

        private readonly GD32_DMA dma;
        private readonly int rxStream, txStream;
        private uint ctl0, ctl1;
        private byte rx;
        private bool rbne, pumping, again, flight;
        private byte txByte;
    }
}
