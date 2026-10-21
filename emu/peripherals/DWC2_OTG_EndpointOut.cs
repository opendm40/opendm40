//
// DWC2 OTG_FS device-mode model for the DM40's GD32F4 USBFS, derived from
// Renode 1.17.0's STM_USB (renamed DesignWare_USB_OTG upstream on
// 2026-09-29), Copyright (c) 2010-2026 Antmicro, MIT License
// (emu/.renode/current/licenses/MIT.txt). Changes for the GD32 drivers:
//   * DTXFSTS reports the programmed Tx FIFO depth (TX0FD / INEPTXFD) minus
//     the words already queued, not the remaining transfer size: the stock
//     driver writes the FIFO only while the free space is LARGER than the
//     packet it wants to send, so it hung on EP0 with exactly 5 free words.
//   * A Bulk-OUT URB longer than the max packet size is split into packets.
//
using Antmicro.Renode.Core.USB;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Utilities;
using Antmicro.Renode.Utilities.Packets;

namespace Antmicro.Renode.Peripherals.USB
{

public partial class DWC2_OTG
{
    private class EndpointOut : Endpoint, IUSBPipeWrite
    {
        private readonly System.Collections.Generic.Queue<byte[]> pending = new System.Collections.Generic.Queue<byte[]>();

        public EndpointOut(DWC2_OTG parent, uint idx) : base(parent, (ulong)Registers.DeviceEpOutRegisters + 0x20 * idx, false, (byte)idx)
        {
            // Intentionally left empty
        }

        public bool MaskMatches(EndpointInterruptMask mask)
        {
            return (mask.TransferComplete && transferCompleteInterruptFlag.Value)
                || (mask.EndpointDisabled && endpointDisabledInterruptFlag.Value)
                || (mask.SetupPhaseDone && setupPhaseDoneInterruptFlag.Value);
        }

        public void SetInterruptsForPacket(RxPacket packet)
        {
            if(packet.Data.Length > 0)
            {
                transferCompleteInterruptFlag.Value = true;
            }
            if(packet.IsSetup)
            {
                setupPhaseDoneInterruptFlag.Value = true;
                setupPacketReceivedInterruptFlag.Value = true;
            }
        }

        public void DropHeld()
        {
            pending.Clear();
        }

        public int MaxPacket => (int)MaxPacketBytes;

        public void Write(byte[] data)
        {
            lock(parent.HostLock)
            {
                WriteLocked(data);
            }
        }

        private void WriteLocked(byte[] data)
        {
            if(!Active)
            {
                parent.WarningLog("Out endpoint #{0} - host tried to write out when endpoint is inactive", endpointNumber);
                return;
            }
            // A USB/IP URB is a whole transfer, not a packet: split it into
            // max-packet-size packets as the host controller would (a 68-byte
            // USBTMC message used to be dropped here as one oversized packet).
            // A transfer that is an exact multiple of the packet size gets no
            // ZLP: the class protocols here (USBTMC, MSC BOT) carry their own
            // lengths.
            var mps = (int)MaxPacketBytes;
            if(data.Length > mps && mps > 0)
            {
                for(var off = 0; off < data.Length; off += mps)
                {
                    var n = System.Math.Min(mps, data.Length - off);
                    var chunk = new byte[n];
                    System.Array.Copy(data, off, chunk, 0, n);
                    WritePacket(chunk);
                }
                return;
            }
            WritePacket(data);
        }

        private void WritePacket(byte[] data)
        {
            if(nakStatusFlag.Value)
            {
                // NAK: the real host retries until CNAK; keep the packet until then.
                parent.DebugLog("Out endpoint #{0} - NAK, holding {1} bytes until CNAK", endpointNumber, data.Length);
                pending.Enqueue(data);
                return;
            }
            parent.DebugLog("Out endpoint #{0} - writing out packet from host - {1}", endpointNumber, data.ToLazyHexString());
            parent.rxFifoPackets.Enqueue(new RxPacket
            {
                IsSetup = false,
                Endpoint = endpointNumber,
                Data = data
            });
            parent.UpdateInterrupts();
        }

        protected override void OnNakCleared()
        {
            while(pending.Count > 0)
            {
                var d = pending.Dequeue();
                parent.DebugLog("Out endpoint #{0} - delivering held packet of {1} bytes", endpointNumber, d.Length);
                parent.rxFifoPackets.Enqueue(new RxPacket { IsSetup = false, Endpoint = endpointNumber, Data = d });
            }
            parent.UpdateInterrupts();
        }

        public void SetupPacketWrite(SetupPacket packet)
        {
            lock(parent.HostLock)
            {
                SetupPacketWriteLocked(packet);
            }
        }

        private void SetupPacketWriteLocked(SetupPacket packet)
        {
            if(!Active)
            {
                parent.WarningLog("Out endpoint #{0} - host tried to write setup when endpoint is inactive", endpointNumber);
                return;
            }
            parent.DebugLog("Out endpoint #{0} - writing setup packet from host - {1}", endpointNumber, packet);
            if(endpointNumber == 0)
            {
                // STALL is cleared when the driver takes it (RxFifoStatusPop).
                parent.ep0SetupsQueued++;
            }
            parent.rxFifoPackets.Enqueue(new RxPacket
            {
                IsSetup = true,
                Endpoint = endpointNumber,
                Data = Packet.Encode(packet)
            });
            parent.UpdateInterrupts();
        }
    }
}
}
