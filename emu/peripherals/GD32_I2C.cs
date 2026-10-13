//
// GD32F4 I2C master (register-compatible with STM32F1/F4 I2C) with the parts
// both DM40 drivers need: START/SBSEND, address/ADDSEND or AERR, data via the
// DATA register with TBE/RBNE/BTC, the two-stage receive pipeline (DATA +
// shift register, clock stretching when both are full), ACKEN/POAP, DMA
// requests (RX one request per byte, TX bytes queued whenever they are
// written) and DMALST. Everything runs on virtual time (about 100 kHz) so
// polling drivers and the stock interrupt+DMA driver both work.
//
// Registers: CTL0 0x00 CTL1 0x04 SADDR0 0x08 SADDR1 0x0C DATA 0x10 STAT0 0x14
// STAT1 0x18 CKCFG 0x1C RT 0x20 FMPCFG 0x90.
//
using System;
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Core.Structure;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.I2C;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.I2C
{
    public class GD32_I2C : SimpleContainer<II2CPeripheral>, IDoubleWordPeripheral, IWordPeripheral, IBytePeripheral, IKnownSize
    {
        public GD32_I2C(IMachine machine, ulong dmaRxCountAddress = 0, ulong byteTimeUs = 90) : base(machine)
        {
            this.mach = machine;
            this.dmaRxCountAddress = dmaRxCountAddress;
            this.byteTimeUs = byteTimeUs;
            Reset();
        }

        public override void Reset()
        {
            ctl0 = 0; ctl1 = 0; saddr0 = 0; saddr1 = 0; ckcfg = 0; rt = 2; fmpcfg = 0;
            stat0 = 0; data = 0;
            txQueue.Clear();
            ResetTransaction();
            UpdateInterrupts();
        }

        public long Size => 0x400;

        public GPIO EventInterrupt { get; } = new GPIO();
        public GPIO ErrorInterrupt { get; } = new GPIO();
        public GPIO RxDMARequest { get; } = new GPIO();
        public GPIO TxDMARequest { get; } = new GPIO();

        public byte ReadByte(long offset) => (byte)ReadDoubleWord(offset);
        public void WriteByte(long offset, byte value) => WriteDoubleWord(offset, value);
        public ushort ReadWord(long offset) => (ushort)ReadDoubleWord(offset);
        public void WriteWord(long offset, ushort value) => WriteDoubleWord(offset, value);

        public uint ReadDoubleWord(long offset)
        {
            switch(offset)
            {
            case 0x00: return ctl0;
            case 0x04: return ctl1;
            case 0x08: return saddr0;
            case 0x0C: return saddr1;
            case 0x10: return ReadData();
            case 0x14:
                stat0Read = true;
                return stat0;
            case 0x18:
                if(stat0Read && (stat0 & ADDSEND) != 0)
                {
                    stat0 &= ~ADDSEND;
                    AddressPhaseDone();
                }
                stat0Read = false;
                return Stat1();
            case 0x1C: return ckcfg;
            case 0x20: return rt;
            case 0x90: return fmpcfg;
            default: return 0;
            }
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            switch(offset)
            {
            case 0x00:
                {
                    var old = ctl0;
                    ctl0 = value & 0xFFFF;
                    if((value & SWRST) != 0)
                    {
                        ctl0 &= ~SWRST;
                        stat0 = 0;
                        txQueue.Clear();
                        ResetTransaction();
                    }
                    if((old & I2CEN) == 0 && (ctl0 & I2CEN) != 0)
                    {
                        stat0 = 0;
                        txQueue.Clear();
                        ResetTransaction();
                    }
                    if((ctl0 & I2CEN) == 0)
                    {
                        ResetTransaction();
                    }
                    if((value & START) != 0 && (old & START) == 0)
                    {
                        Schedule(10, GenerateStart);
                    }
                    if((value & STOP) != 0 && (old & STOP) == 0)
                    {
                        Schedule(10, GenerateStop);
                    }
                    UpdateInterrupts();
                }
                break;
            case 0x04:
                {
                    var old = ctl1;
                    ctl1 = value & 0x1FFF;
                    UpdateInterrupts();
                    if((ctl1 & DMAON) != 0 && (old & DMAON) == 0 && phase == Phase.Tx && txQueue.Count == 0 && !txRunning)
                    {
                        TxDMARequest.Blink();
                    }
                }
                break;
            case 0x08: saddr0 = value; break;
            case 0x0C: saddr1 = value; break;
            case 0x10: WriteData((byte)value); break;
            case 0x14:
                // rc_w0 error flags
                stat0 &= value | ~(BERR | LOSTARB | AERR | OUERR | PECERR | SMBTO | SMBALT);
                UpdateInterrupts();
                break;
            case 0x1C: ckcfg = value; break;
            case 0x20: rt = value; break;
            case 0x90: fmpcfg = value; break;
            }
        }

        // ---- transaction engine ----
        private void GenerateStart()
        {
            if((ctl0 & I2CEN) == 0)
            {
                ctl0 &= ~START;
                return;
            }
            if(busy && child != null && (phase == Phase.Tx || phase == Phase.Rx))
            {
                child.FinishTransmission(); // repeated start ends the previous transfer
            }
            busy = true;
            phase = Phase.Address;
            ctl0 &= ~START;
            stat0 |= SBSEND;
            stat0 &= ~(TBE | RBNE | BTC);
            UpdateInterrupts();
        }

        private void GenerateStop()
        {
            this.Log(LogLevel.Debug, "stop");
            if(child != null && busy)
            {
                child.FinishTransmission();
            }
            ctl0 &= ~STOP;
            ResetTransaction();
            UpdateInterrupts();
        }

        private void WriteData(byte value)
        {
            if(phase == Phase.Address && (stat0 & SBSEND) != 0)
            {
                stat0 &= ~SBSEND;
                var addr = value >> 1;
                readMode = (value & 1) != 0;
                if(!TryGetByAddress(addr, out child))
                {
                    child = null;
                }
                Schedule(byteTimeUs, () =>
                {
                    if(child == null)
                    {
                        // No ACK: the master releases the bus (as the stock relies on).
                        this.Log(LogLevel.Debug, "no device at 0x{0:X2}", addr);
                        stat0 |= AERR;
                        ResetTransaction();
                        UpdateInterrupts();
                        return;
                    }
                    this.Log(LogLevel.Debug, "address 0x{0:X2} {1}", addr, readMode ? "read" : "write");
                    stat0 |= ADDSEND;
                    if(!readMode)
                    {
                        stat1Tr = true;
                    }
                    UpdateInterrupts();
                });
                return;
            }
            // Data byte (possibly pushed early by the DMA): queue it.
            txQueue.Enqueue(value);
            stat0 &= ~TBE;
            if(phase == Phase.Tx && !txRunning)
            {
                PumpTx();
            }
        }

        private void AddressPhaseDone()
        {
            if(readMode)
            {
                phase = Phase.Rx;
                stat1Tr = false;
                shiftFull = false;
                dataFull = false;
                nacked = false;
                stat0 &= ~(RBNE | BTC);
                Schedule(byteTimeUs, ReceiveByte);
            }
            else
            {
                phase = Phase.Tx;
                stat1Tr = true;
                stat0 |= TBE;
                if(txQueue.Count > 0)
                {
                    PumpTx();
                }
                else if((ctl1 & DMAON) != 0)
                {
                    TxDMARequest.Blink();
                }
            }
        }

        private void PumpTx()
        {
            txRunning = true;
            Schedule(byteTimeUs, () =>
            {
                txRunning = false;
                if(phase != Phase.Tx)
                {
                    return;
                }
                if(txQueue.Count > 0)
                {
                    var b = txQueue.Dequeue();
                    this.Log(LogLevel.Debug, "tx 0x{0:X2}", b);
                    child?.Write(new[] { b });
                    stat0 |= TBE;
                    PumpTx();
                    UpdateInterrupts();
                    if((ctl1 & DMAON) != 0 && txQueue.Count == 0)
                    {
                        TxDMARequest.Blink();
                    }
                }
                else
                {
                    // Shift register empty too: byte transfer complete.
                    stat0 |= TBE | BTC;
                    UpdateInterrupts();
                    if((ctl1 & DMAON) != 0)
                    {
                        TxDMARequest.Blink();
                    }
                }
            });
        }

        private bool AckForNextByte()
        {
            if((ctl0 & ACKEN) != 0)
            {
                return true;
            }
            if((ctl0 & POAP) != 0 && !poapUsed)
            {
                poapUsed = true; // POAP: the NACK applies to the following byte
                return true;
            }
            return false;
        }

        private void ReceiveByte()
        {
            if(phase != Phase.Rx || nacked)
            {
                return;
            }
            if(shiftFull)
            {
                // Clock stretching: wait for the DATA register to be read.
                stretching = true;
                return;
            }
            stretching = false;
            var ack = AckForNextByte();
            if(dmaLastCheck && (ctl1 & DMAON) != 0 && (ctl1 & DMALST) != 0 && DmaRemaining() == 1)
            {
                ack = false; // DMALST: NACK the last DMA byte
            }
            var b = child != null ? child.Read(1) : new byte[] { 0xFF };
            shiftReg = b.Length > 0 ? b[0] : (byte)0xFF;
            this.Log(LogLevel.Debug, "rx 0x{0:X2} ack={1}", shiftReg, ack);
            shiftFull = true;
            if(!ack)
            {
                nacked = true;
            }
            MoveShiftToData();
            if(!nacked)
            {
                Schedule(byteTimeUs, ReceiveByte);
            }
        }

        private void MoveShiftToData()
        {
            if(!shiftFull)
            {
                return;
            }
            if(!dataFull)
            {
                data = shiftReg;
                shiftFull = false;
                dataFull = true;
                stat0 |= RBNE;
                stat0 &= ~BTC;
                UpdateInterrupts();
                if((ctl1 & DMAON) != 0)
                {
                    RxDMARequest.Blink();
                }
            }
            else
            {
                stat0 |= BTC; // DATA and shift register both full
                UpdateInterrupts();
            }
        }

        private uint ReadData()
        {
            var v = data;
            if(phase == Phase.Rx)
            {
                dataFull = false;
                stat0 &= ~(RBNE | BTC);
                if(shiftFull)
                {
                    MoveShiftToData();
                }
                if(stretching && !nacked)
                {
                    stretching = false;
                    Schedule(byteTimeUs, ReceiveByte);
                }
                UpdateInterrupts();
            }
            return v;
        }

        private int DmaRemaining()
        {
            if(dmaRxCountAddress == 0)
            {
                return int.MaxValue;
            }
            return (int)(mach.SystemBus.ReadDoubleWord(dmaRxCountAddress) & 0xFFFF);
        }

        private void ResetTransaction()
        {
            busy = false;
            phase = Phase.Idle;
            stat1Tr = false;
            shiftFull = false; dataFull = false; nacked = false; stretching = false; poapUsed = false; txRunning = false;
            stat0 &= ~(SBSEND | ADDSEND | BTC | RBNE | TBE);
            TxDMARequest.Set(false);
        }

        private uint Stat1()
        {
            uint v = 0;
            if(busy) v |= MASTER | I2CBSY;
            if(stat1Tr) v |= TR;
            return v;
        }

        private void UpdateInterrupts()
        {
            var evie = (ctl1 & EVIE) != 0;
            var bufie = (ctl1 & BUFIE) != 0;
            var ev = evie && ((stat0 & (SBSEND | ADDSEND | BTC | STPDET)) != 0 || (bufie && (stat0 & (TBE | RBNE)) != 0));
            var er = (ctl1 & ERRIE) != 0 && (stat0 & (BERR | LOSTARB | AERR | OUERR | PECERR | SMBTO | SMBALT)) != 0;
            EventInterrupt.Set(ev);
            ErrorInterrupt.Set(er);
        }

        private void Schedule(ulong us, Action a)
        {
            mach.ScheduleAction(TimeInterval.FromMicroseconds(us), _ => a());
        }

        private enum Phase { Idle, Address, Tx, Rx }

        private const uint I2CEN = 1 << 0, START = 1 << 8, STOP = 1 << 9, ACKEN = 1 << 10, POAP = 1 << 11, SWRST = 1 << 15;
        private const uint ERRIE = 1 << 8, EVIE = 1 << 9, BUFIE = 1 << 10, DMAON = 1 << 11, DMALST = 1 << 12;
        private const uint SBSEND = 1 << 0, ADDSEND = 1 << 1, BTC = 1 << 2, STPDET = 1 << 4, RBNE = 1 << 6, TBE = 1 << 7;
        private const uint BERR = 1 << 8, LOSTARB = 1 << 9, AERR = 1 << 10, OUERR = 1 << 11, PECERR = 1 << 12, SMBTO = 1 << 14, SMBALT = 1 << 15;
        private const uint MASTER = 1 << 0, I2CBSY = 1 << 1, TR = 1 << 2;

        private readonly IMachine mach;
        private readonly ulong dmaRxCountAddress;
        private readonly ulong byteTimeUs;
        private readonly Queue<byte> txQueue = new Queue<byte>();
        private const bool dmaLastCheck = true;
        private uint ctl0, ctl1, saddr0, saddr1, ckcfg, rt, fmpcfg, stat0, data;
        private bool stat0Read, busy, readMode, stat1Tr, shiftFull, dataFull, nacked, stretching, poapUsed, txRunning;
        private byte shiftReg;
        private Phase phase;
        private II2CPeripheral child;
    }
}
