//
// DWC2 OTG_FS device-mode model for the DM40's GD32F4 USBFS, derived from
// Renode 1.17.0's STM_USB (renamed DesignWare_USB_OTG upstream on
// 2026-09-29), Copyright (c) 2010-2026 Antmicro, MIT License
// (emu/.renode/current/licenses/MIT.txt). Changes for the GD32 drivers:
//   * DTXFSTS reports the programmed Tx FIFO depth (TX0FD / INEPTXFD) minus
//     the words already queued, not the remaining transfer size: the stock
//     driver writes the FIFO only while the free space is LARGER than the
//     packet it wants to send, so it hung on EP0 with exactly 5 free words.
//   * A packet read by the host clears EPENA before it raises XFRC.
//   * The host takes one packet at a time (XFRSIZ counts down), so a
//     transfer larger than the Tx FIFO completes.
//
using System;
using System.Collections.Generic;

using Antmicro.Renode.Core.Structure.Registers;
using Antmicro.Renode.Core.USB;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Utilities;

namespace Antmicro.Renode.Peripherals.USB
{

public partial class DWC2_OTG
{
    private class EndpointIn : Endpoint, IUSBPipeRead
    {
        public EndpointIn(DWC2_OTG parent, uint idx, IFlagRegisterField fifoEmptyInterruptMaskFlag) : base(parent, (ulong)Registers.DeviceEpInRegisters + 0x20 * idx, true, (byte)idx)
        {
            this.fifoEmptyInterruptMaskFlag = fifoEmptyInterruptMaskFlag;
        }

        public bool MaskMatches(EndpointInterruptMask mask)
        {
            return (mask.TransferComplete && transferCompleteInterruptFlag.Value)
                || (mask.EndpointDisabled && endpointDisabledInterruptFlag.Value)
                || (fifoEmptyInterruptMaskFlag.Value && txFifoEmptyInterruptFlag.Value);
        }

        public void Flush()
        {
            fifo.Clear();
        }

        public int MaxPacket => (int)MaxPacketBytes;

        public void SubmitData(uint data)
        {
            var bytesToTake = Math.Min(FifoAvailableSpace, sizeof(uint));
            var bytes = BitHelper.GetBytesFromValue(data, sizeof(uint), reverse: true)[0..(int)bytesToTake];
            fifo.EnqueueRange(bytes);
            parent.DebugLog("In endpoint #{0} - got {1:X08} from device, taking {2}", endpointNumber, data, bytesToTake);

            if(fifo.Count >= NextPacketBytes)
            {
                parent.DebugLog("In endpoint #{0} - got full packet from device, notifying host", endpointNumber);
                parent.QueueNewPacket(this);
            }
            UpdateTxFifoEmpty();
            parent.UpdateInterrupts();
        }

        /// The next packet of the transfer: max packet size or what is left.
        private int NextPacketBytes => (int)Math.Min((ulong)Math.Max(1u, MaxPacketBytes), transferSizeField.Value);

        /// TXFE: more of the transfer to write and the Tx FIFO at least half
        /// empty (as the core does). A transfer larger than the FIFO (the
        /// stock's 4 KiB MSC reads) is written in parts while the host takes
        /// packets; it used to wait for the whole transfer to be in the FIFO,
        /// which never fit.
        private void UpdateTxFifoEmpty()
        {
            var depthBytes = (int)parent.TxFifoDepthWords(endpointNumber) * 4;
            txFifoEmptyInterruptFlag.Value = fifo.Count < (int)transferSizeField.Value
                && (depthBytes == 0 || fifo.Count * 2 <= depthBytes);
        }

        public bool TryRead(out byte[] data)
        {
            // Host side (USB/IP server thread): serialised with CPU register
            // accesses (DWC2_OTG.HostLock).
            lock(parent.HostLock)
            {
                return TryReadLocked(out data);
            }
        }

        private bool TryReadLocked(out byte[] data)
        {
            data = null;
            if(!Active)
            {
                parent.WarningLog("In endpoint #{0} - host tried to read when endpoint is inactive", endpointNumber);
                return false;
            }
            if(!endpointEnabledFlag.Value || nakStatusFlag.Value)
            {
                parent.DebugLog("In endpoint #{0} - host tried to read an empty endpoint", endpointNumber);
                return false;
            }
            parent.DebugLog("In endpoint #{0} - host tried to read at {1}/{2} (max: {3})", endpointNumber, fifo.Count, transferSizeField.Value, maxPacketSize.Value);
            // One packet per IN token, as on the bus: XFRSIZ counts down.
            var n = NextPacketBytes;
            if(fifo.Count < n)
            {
                return false;
            }
            data = fifo.DequeueRange(n);
            transferSizeField.Value -= (ulong)n;
            if(transferSizeField.Value == 0)
            {
                // EPENA first, as the core does: the driver re-arms the
                // endpoint as soon as it sees XFRC.
                endpointEnabledFlag.Value = false;
                txFifoEmptyInterruptFlag.Value = true;
                transferCompleteInterruptFlag.Value = true;
            }
            else
            {
                UpdateTxFifoEmpty();
            }
            parent.UpdateInterrupts();
            return true;
        }

        public event Action NewPacket;

        /// Raised by DWC2_OTG after it released HostLock (the server's
        /// handler may wait for its own thread, which takes the lock).
        public void FireNewPacket()
        {
            NewPacket?.Invoke();
        }

        // Wake the host side waiting for a packet: it reports the STALL.
        protected override void OnStallSet()
        {
            parent.QueueNewPacket(this);
        }

        protected override void OnEndpointEnable()
        {
            if(transferSizeField.Value == 0)
            {
                parent.QueueNewPacket(this);
            }
            UpdateTxFifoEmpty();
            parent.UpdateInterrupts();
        }

        protected override ulong FifoAvailableSpace => (ulong)Math.Max(0, (int)transferSizeField.Value - fifo.Count);

        protected override ulong TxFifoFreeWords
        {
            get
            {
                var depth = (long)parent.TxFifoDepthWords(endpointNumber);
                var used = (fifo.Count + 3) / 4;
                return (ulong)Math.Max(0, depth - used);
            }
        }

        private readonly Queue<byte> fifo = new();

        private readonly IFlagRegisterField fifoEmptyInterruptMaskFlag;
    }
}
}
