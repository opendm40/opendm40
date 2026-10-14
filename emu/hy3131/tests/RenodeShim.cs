// Minimal stand-ins for the Renode types used by emu/peripherals/HY3131.cs so the
// real model file compiles on the host (dotnet 8, no Renode assemblies).
using System;
using System.Collections.Generic;
using System.Collections.ObjectModel;

namespace Antmicro.Renode.Core
{
    public interface IGPIOReceiver { void OnGPIO(int number, bool value); }

    public interface IGPIO
    {
        void Connect(IGPIOReceiver receiver, int number);
        void Set(bool value);
        bool IsSet { get; }
    }

    public class GPIO : IGPIO
    {
        public void Connect(IGPIOReceiver receiver, int number) { targets.Add(new KeyValuePair<IGPIOReceiver, int>(receiver, number)); }
        public void Set(bool value) { IsSet = value; foreach(var t in targets) t.Key.OnGPIO(t.Value, value); }
        public bool IsSet { get; private set; }
        private readonly List<KeyValuePair<IGPIOReceiver, int>> targets = new List<KeyValuePair<IGPIOReceiver, int>>();
    }

    public interface INumberedGPIOOutput { IReadOnlyDictionary<int, IGPIO> Connections { get; } }

    public class ElapsedTime { public TimeSpan TimeElapsed; }

    public class SystemBusStub
    {
        public byte[] Flash = new byte[0];
        public ulong FlashBase = 0x08000000;
        public byte[] ReadBytes(ulong address, int count)
        {
            var r = new byte[count];
            var off = (long)(address - FlashBase);
            if(off < 0 || off + count > Flash.Length) throw new Exception("unmapped");
            Array.Copy(Flash, off, r, 0, count);
            return r;
        }
    }

    public interface IMachine
    {
        ElapsedTime ElapsedVirtualTime { get; }
        SystemBusStub SystemBus { get; }
    }

    public class MachineStub : IMachine
    {
        public ElapsedTime ElapsedVirtualTime { get; } = new ElapsedTime();
        public SystemBusStub SystemBus { get; } = new SystemBusStub();
        public double Now
        {
            get => ElapsedVirtualTime.TimeElapsed.TotalSeconds;
            set => ElapsedVirtualTime.TimeElapsed = TimeSpan.FromSeconds(value);
        }
    }
}

namespace Antmicro.Renode.Logging
{
    public enum LogLevel { Noisy, Debug, Info, Warning, Error }
    public static class LogExt
    {
        public static bool Verbose;
        public static void Log(this object o, LogLevel level, string fmt, params object[] args)
        {
            if(Verbose || level >= LogLevel.Warning) Console.WriteLine("[{0}] {1}", level, string.Format(fmt, args));
        }
    }
}

namespace Antmicro.Renode.Peripherals
{
    public interface IKnownSize { long Size { get; } }
}

namespace Antmicro.Renode.Peripherals.Bus
{
    public interface IDoubleWordPeripheral { uint ReadDoubleWord(long offset); void WriteDoubleWord(long offset, uint value); }
}
