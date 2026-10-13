//
// Helper for the monitor: save a system-bus range to a file (flash write-back,
// RAM dumps), fill a range, and print a few words.
//
using System;
using System.IO;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals;
using Antmicro.Renode.Peripherals.Bus;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class MemTool : IDoubleWordPeripheral, IKnownSize
    {
        public MemTool(IMachine machine)
        {
            this.machine = machine;
        }

        public void Reset()
        {
        }

        public long Size => 0x100;

        public uint ReadDoubleWord(long offset)
        {
            return 0;
        }

        public void WriteDoubleWord(long offset, uint value)
        {
        }

        public void Save(string path, ulong address, int size)
        {
            var d = machine.SystemBus.ReadBytes(address, size);
            File.WriteAllBytes(path, d);
            this.Log(LogLevel.Info, "saved 0x{0:X} bytes from 0x{1:X8} to {2}", size, address, path);
        }

        public void Fill(ulong address, int size, byte value)
        {
            var d = new byte[size];
            for(var i = 0; i < size; i++) d[i] = value;
            machine.SystemBus.WriteBytes(d, address);
        }

        private readonly IMachine machine;
    }
}
