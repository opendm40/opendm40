//
// Bluetooth SPP/BLE module on UART4 as the DM40 firmware expects it
// (firmware/re/analysis/bt_meas_v2_6.h, fw-rs/src/bt.rs): answers AT lines
// ("AT+SBN<name>" -> OK, "AT+CBN" -> "CBN+<name>", "AT+SLED0"/"AT+RESET" ->
// OK, anything else "AT..." -> OK). PC14 ("connected") is driven high while a
// host is attached to the TCP port. With a port, a TCP server passes the
// firmware's UART bytes to the host and the host's bytes to the firmware, so
// the stock measurement protocol (AF 05 03 ...) and fw-rs's SCPI can be used
// from the host; AT lines are still answered locally.
//
using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.Bus;
using Antmicro.Renode.Peripherals.UART;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.Miscellaneous
{
    public class BtModule : IDoubleWordPeripheral, IKnownSize
    {
        public BtModule(IMachine machine, IUART uart, IGPIOReceiver connPort = null, int connPin = 14)
        {
            this.machine = machine;
            this.uart = uart;
            this.connPort = connPort;
            this.connPin = connPin;
            uart.CharReceived += OnChar;
            Name = "DM40A";
            Reset();
        }

        public void Reset()
        {
            line.Clear();
            inAt = false;
        }

        public long Size => 0x100;
        public uint ReadDoubleWord(long offset) => 0;
        public void WriteDoubleWord(long offset, uint value) { }

        public string Name { get; set; }
        public ulong AtCommands { get; private set; }
        public ulong Lines { get; private set; }

        /// Host side: start a TCP server; bytes are passed both ways.
        public void Listen(int port)
        {
            listener = new TcpListener(IPAddress.Loopback, port);
            listener.Start();
            this.Log(LogLevel.Info, "BT passthrough listening on 127.0.0.1:{0}", port);
            var t = new Thread(AcceptLoop) { IsBackground = true, Name = "bt-accept" };
            t.Start();
        }

        /// Script side: raise/drop the module's "connected" pin (PC14) without a TCP host.
        public void SetConnected(bool on)
        {
            connPort?.OnGPIO(connPin, on);
        }

        /// Script side: inject bytes as if the host sent them (hex string, e.g. "AF0503000000..").
        public void SendHex(string hex)
        {
            var bytes = new List<byte>();
            for(var i = 0; i + 1 < hex.Length; i += 2)
            {
                bytes.Add(Convert.ToByte(hex.Substring(i, 2), 16));
            }
            SendToUart(bytes.ToArray());
        }

        public void SendText(string text)
        {
            SendToUart(Encoding.ASCII.GetBytes(text));
        }

        /// Injection goes through a timed action so that a monitor command (also
        /// one issued while the emulation is paused) never blocks on the UART's
        /// receive path; the bytes arrive once the emulation runs.
        private void SendToUart(byte[] bytes)
        {
            machine.ScheduleAction(TimeInterval.FromMicroseconds(1), _ =>
            {
                foreach(var b in bytes)
                {
                    uart.WriteChar(b);
                }
            });
        }

        // ---- firmware -> module ----
        private void OnChar(byte b)
        {
            TxLog(b);
            // Pass through to the host first.
            var c = client;
            if(c != null)
            {
                try { c.GetStream().WriteByte(b); } catch(Exception) { client = null; }
            }
            // AT parser: lines starting with "AT" terminated by \r\n (or 50 ms idle for
            // the stock's "AT+SBNDM40A" without a terminator).
            if(line.Count == 0 && b != (byte)'A') return;
            if(line.Count == 1 && b != (byte)'T') { line.Clear(); return; }
            line.Add(b);
            if(b == (byte)'\n' || b == (byte)'\r')
            {
                Complete();
            }
            else
            {
                idleGen++;
                var g = idleGen;
                machine.ScheduleAction(TimeInterval.FromMilliseconds(20), _ => { if(g == idleGen && line.Count > 2) Complete(); });
            }
        }

        private void Complete()
        {
            var s = Encoding.ASCII.GetString(line.ToArray()).Trim();
            line.Clear();
            if(s.Length < 2) return;
            AtCommands++;
            this.Log(LogLevel.Debug, "AT command: {0}", s);
            string reply;
            if(s.StartsWith("AT+SBN"))
            {
                Name = s.Substring(6);
                reply = "OK";
            }
            else if(s.StartsWith("AT+CBN"))
            {
                reply = "CBN+" + Name;
            }
            else
            {
                reply = "OK";
            }
            var r = Encoding.ASCII.GetBytes(reply + "\r\n");
            // Reply after a short module delay so the firmware sees a separate answer.
            machine.ScheduleAction(TimeInterval.FromMilliseconds(5), _ => SendToUart(r));
        }

        private void TxLog(byte b)
        {
            if(b == (byte)'\n' || txLine.Count > 200)
            {
                this.Log(LogLevel.Debug, "fw -> host: {0}", Encoding.ASCII.GetString(txLine.ToArray()).TrimEnd('\r'));
                txLine.Clear();
            }
            else
            {
                txLine.Add(b);
            }
        }

        // ---- host socket ----
        private void AcceptLoop()
        {
            while(true)
            {
                TcpClient c;
                try { c = listener.AcceptTcpClient(); } catch(Exception) { return; }
                this.Log(LogLevel.Info, "BT host connected");
                client = c;
                connPort?.OnGPIO(connPin, true);
                var buf = new byte[256];
                try
                {
                    var st = c.GetStream();
                    int n;
                    while((n = st.Read(buf, 0, buf.Length)) > 0)
                    {
                        var copy = new byte[n];
                        Array.Copy(buf, copy, n);
                        SendToUart(copy); // delivered on the emulation thread
                    }
                }
                catch(Exception) { }
                client = null;
                connPort?.OnGPIO(connPin, false);
                this.Log(LogLevel.Info, "BT host disconnected");
            }
        }

        private readonly IMachine machine;
        private readonly IUART uart;
        private readonly IGPIOReceiver connPort;
        private readonly int connPin;
        private readonly List<byte> line = new List<byte>();
        private readonly List<byte> txLine = new List<byte>();
        private bool inAt;
        private int idleGen;
        private TcpListener listener;
        private volatile TcpClient client;
    }
}
