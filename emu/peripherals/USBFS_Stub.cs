//
// USB OTG FS (Synopsys DWC2) stub at 0x50000000: register storage with the
// handshakes the DM40 firmware polls answered so that nothing spins:
// GRSTCTL.AHBIDL always set and the soft-reset bits self-clearing,
// GINTSTS reads 0 (no events, so no enumeration ever happens), endpoint
// EPENA bits self-clear. Good enough for the stock app, the FS-loader's MSC
// bring-up and fw-rs's USBTMC/MSC init; no host is modelled.
//
using System;
using Antmicro.Renode.Core;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.USB
{
    public class USBFS_Stub : IDoubleWordPeripheral, IKnownSize
    {
        public USBFS_Stub(IMachine machine)
        {
            Reset();
        }

        public void Reset()
        {
            Array.Clear(regs, 0, regs.Length);
            regs[0x40 / 4] = 0x4F54300A; // GSNPSID
            regs[0x44 / 4] = 0x00000000;
            regs[0x48 / 4] = 0x00000000;
        }

        public long Size => 0x40000;

        public uint ReadDoubleWord(long offset)
        {
            if(offset >= regs.Length * 4) return 0;
            var v = regs[offset / 4];
            switch(offset)
            {
            case 0x10: return (v & ~0x3Fu) | 0x80000000; // GRSTCTL: AHBIDL, resets done
            case 0x14: return 0; // GINTSTS: no events
            case 0x1C: case 0x20: return 0; // GRXSTSR/P
            case 0x3C: return v | 0x3; // GGPIO-ish: nothing pending
            }
            if(offset >= 0x900 && offset < 0xD00 && (offset & 0x1F) == 0)
            {
                return v & ~0x80000000; // DIEPCTL/DOEPCTL: EPENA self-clears
            }
            if(offset >= 0x900 && offset < 0xD00 && (offset & 0x1F) == 0x08)
            {
                return 0; // DxEPINT: nothing
            }
            return v;
        }

        public void WriteDoubleWord(long offset, uint value)
        {
            if(offset >= regs.Length * 4) return;
            regs[offset / 4] = value;
        }

        private readonly uint[] regs = new uint[0x1000 / 4];
    }
}
