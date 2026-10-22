//
// Winbond W25Q128JV (16 MiB SPI NOR) as used on the DM40 for the FAT disk.
// File-backed: Load(path) copies an image in, Save(path) writes it out; the
// emulator never touches the source image. Chip select is a GPIO input
// (PE4 on the DM40, software driven) connected from the port in the constructor.
// Commands: READ 03, FAST READ 0B, PP 02, SE 20, BE32 52, BE64 D8, CE C7/60,
// WREN 06, WRDI 04, RDSR1 05, RDSR2 35, RDSR3 15, WRSR 01, REMS 90, JEDEC 9F,
// RUID 4B, power-down/release/reset as no-ops.
// Busy model (off by default = never busy): ProgramUs / Erase4kUs /
// Erase64kUs / Erase32kUs set how long RDSR1 reports BUSY (bit 0) after a
// page program / erase, in virtual time (datasheet W25Q128JV: tPP 0.4 ms
// typ / 3 ms max, tSE 45 / 400 ms, tBE1 120 / 1600 ms, tBE2 150 / 2000 ms).
// A command other than RDSR while busy is counted (BusyViolations) and
// still executed. The SPI byte time is not modelled (the bus is instant).
//
using System;
using System.IO;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.SPI;

namespace Antmicro.Renode.Peripherals.SPI
{
    public class W25Q128 : ISPIPeripheral, IGPIOReceiver
    {
        public W25Q128(IMachine machine, INumberedGPIOOutput csPort = null, int csPin = 0)
        {
            this.machine = machine;
            mem = new byte[Size];
            for(var i = 0; i < mem.Length; i++) mem[i] = 0xFF;
            if(csPort != null)
            {
                csPort.Connections[csPin].Connect(this, 0);
                selected = false;
            }
            else
            {
                selected = true; // no CS wired: every byte belongs to one transaction
            }
            Reset();
        }

        public void Reset()
        {
            state = State.Cmd; cmd = 0; addr = 0; nAddr = 0; wel = false; dummy = 0;
            Array.Clear(page, 0, page.Length); pageLen = 0;
        }

        public void OnGPIO(int number, bool value)
        {
            var sel = !value; // active low
            if(selected && !sel)
            {
                FinishTransmission();
            }
            selected = sel;
            if(sel)
            {
                state = State.Cmd;
            }
        }

        public byte Transmit(byte data)
        {
            if(!selected)
            {
                return 0xFF;
            }
            switch(state)
            {
            case State.Cmd:
                return StartCommand(data);
            case State.Addr:
                addr = (addr << 8) | data;
                if(++nAddr == 3)
                {
                    addr &= Size - 1;
                    switch(cmd)
                    {
                    case 0x03: state = State.Read; break;
                    case 0x0B: state = State.Dummy; dummy = 1; break;
                    case 0x02: state = State.Program; pageLen = 0; pageBase = addr; break;
                    case 0x20: Erase(addr & ~0xFFFu, 0x1000); state = State.Done; break;
                    case 0x52: Erase(addr & ~0x7FFFu, 0x8000); state = State.Done; break;
                    case 0xD8: Erase(addr & ~0xFFFFu, 0x10000); state = State.Done; break;
                    case 0x90: state = State.Rems; dummy = 0; break;
                    default: state = State.Done; break;
                    }
                }
                return 0xFF;
            case State.Dummy:
                if(--dummy <= 0) state = State.Read;
                return 0xFF;
            case State.Read:
                {
                    var b = mem[addr];
                    addr = (addr + 1) & (Size - 1);
                    return b;
                }
            case State.Program:
                if(pageLen < 256)
                {
                    page[pageLen++] = data;
                }
                return 0xFF;
            case State.Status1:
                return (byte)((wel ? 0x02 : 0x00) | (Now < busyUntil ? 0x01 : 0x00));
            case State.Status2:
                return 0x00;
            case State.Status3:
                return 0x60;
            case State.Jedec:
                {
                    var r = dummy < 3 ? Jedec[dummy] : (byte)0;
                    dummy++;
                    return r;
                }
            case State.Rems:
                {
                    var r = (dummy & 1) == 0 ? (byte)0xEF : (byte)0x17;
                    dummy++;
                    return r;
                }
            case State.Ruid:
                {
                    var r = dummy < 4 ? (byte)0xFF : Uid[(dummy - 4) & 7];
                    dummy++;
                    return r;
                }
            default:
                return 0xFF;
            }
        }

        public void FinishTransmission()
        {
            if(state == State.Program && wel && pageLen > 0)
            {
                // Page program: AND into the page, wrapping inside the 256-byte page.
                for(var i = 0; i < pageLen; i++)
                {
                    var a = (pageBase & ~0xFFu) | ((pageBase + (uint)i) & 0xFF);
                    mem[a] &= page[i];
                }
                Writes++;
                ProgramBytes += (ulong)pageLen;
                Busy(ProgramUs);
            }
            if(cmd == 0x02 || cmd == 0x20 || cmd == 0x52 || cmd == 0xD8 || cmd == 0xC7 || cmd == 0x60 || cmd == 0x01)
            {
                wel = false;
            }
            state = State.Cmd;
        }

        // ---- monitor API ----
        public void Load(string path)
        {
            var d = File.ReadAllBytes(path);
            Array.Copy(d, 0, mem, 0, Math.Min(d.Length, mem.Length));
            this.Log(LogLevel.Info, "loaded {0} bytes from {1}", d.Length, path);
        }

        public void Save(string path)
        {
            File.WriteAllBytes(path, mem);
            this.Log(LogLevel.Info, "saved image to {0} ({1} page programs, {2} erases)", path, Writes, Erases);
        }

        public ulong Writes { get; private set; }
        public ulong Erases { get; private set; }

        private double Now => machine.ElapsedVirtualTime.TimeElapsed.TotalSeconds * 1e6;

        private void Busy(double us)
        {
            if(us > 0)
            {
                busyUntil = Now + us;
            }
        }

        private byte StartCommand(byte c)
        {
            if(c != 0x05 && Now < busyUntil)
            {
                BusyViolations++;
            }
            cmd = c; addr = 0; nAddr = 0;
            switch(c)
            {
            case 0x03: case 0x0B: case 0x02: case 0x20: case 0x52: case 0xD8: case 0x90:
                state = State.Addr; break;
            case 0x05: state = State.Status1; break;
            case 0x35: state = State.Status2; break;
            case 0x15: state = State.Status3; break;
            case 0x06: wel = true; state = State.Done; break;
            case 0x04: wel = false; state = State.Done; break;
            case 0x9F: state = State.Jedec; dummy = 0; break;
            case 0x4B: state = State.Ruid; dummy = 0; break;
            case 0xC7: case 0x60:
                if(wel) { Erase(0, Size); }
                state = State.Done; break;
            case 0x01: state = State.Done; break; // WRSR: ignore
            default:
                state = State.Done; break; // AB, B9, 66, 99, E9, ...
            }
            return 0xFF;
        }

        private void Erase(uint a, uint len)
        {
            if(!wel) return;
            for(uint i = 0; i < len && a + i < Size; i++) mem[a + i] = 0xFF;
            Erases++;
            Busy(len == 0x1000 ? Erase4kUs : len == 0x8000 ? Erase32kUs : len == 0x10000 ? Erase64kUs : Erase64kUs * 256);
        }

        public double ProgramUs { get; set; }
        public double Erase4kUs { get; set; }
        public double Erase32kUs { get; set; }
        public double Erase64kUs { get; set; }
        public ulong ProgramBytes { get; private set; }
        public ulong BusyViolations { get; private set; }

        /// Monitor: "Stats" -> programs, bytes programmed, erases, busy violations.
        public string Stats => string.Format("programs {0} bytes {1} erases {2} busy_violations {3}", Writes, ProgramBytes, Erases, BusyViolations);

        private enum State { Cmd, Addr, Dummy, Read, Program, Status1, Status2, Status3, Jedec, Rems, Ruid, Done }

        private const uint Size = 16 * 1024 * 1024;
        private static readonly byte[] Jedec = { 0xEF, 0x40, 0x18 };
        private static readonly byte[] Uid = { 0xD1, 0x40, 0x00, 0x11, 0x22, 0x33, 0x44, 0x55 };

        private readonly byte[] mem;
        private readonly byte[] page = new byte[256];
        private State state;
        private byte cmd;
        private uint addr, pageBase;
        private int nAddr, pageLen, dummy;
        private bool wel, selected;
        private double busyUntil;
        private readonly IMachine machine;
    }
}
