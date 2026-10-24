//
// USB/IP server for the DWC2 OTG_FS model (replaces Renode's
// `emulation CreateUSBIPServer` + `host.usb Register`, which only works with a
// client that asks exact lengths one URB at a time). Started from the monitor:
//
//     sysbus.usbfs StartUSBIPServer 3240
//
// Exports the device as busid 1-0 (devid 0x10000), like Renode's server, so
// `usbip attach -r 127.0.0.1 -b 1-0` and emu/usbip_client.py work unchanged.
//
// What the Linux vhci-hcd needs and Renode 1.17's USBIPServer lacks:
//   * a control IN transfer ends on a short packet, not only when wLength
//     bytes arrived (the kernel's first request is GET_DESCRIPTOR(device, 64),
//     strings are read with wLength 255: Renode waited forever);
//   * the socket reader never blocks: every endpoint has its own worker and
//     queue, a pending Bulk/Interrupt-IN URB does not hold EP0 or the others;
//   * CMD_UNLINK gets RET_UNLINK (-ECONNRESET if the URB was still pending,
//     0 if it had completed); the kernel unlinks every URB that times out;
//   * STALL completes the URB with -EPIPE; URB_SHORT_NOT_OK gives -EREMOTEIO
//     on a short IN (usb-storage's scatter-gather reads rely on it);
//     URB_ZERO_PACKET appends a ZLP;
//   * IMPORT is a bus reset + SET_ADDRESS(1) (the firmware re-enumerates;
//     vhci-hcd never forwards SET_ADDRESS), DEVLIST of an
//     imported device answers from the cached descriptors;
//   * the speed is reported as full (the GD32 USBFS core), not high.
//
using System;
using System.Collections.Generic;
using System.Net;
using System.Net.Sockets;
using System.Threading;

using Antmicro.Renode.Core.USB;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Utilities.Packets;

namespace Antmicro.Renode.Peripherals.USB
{

public partial class DWC2_OTG
{
    /// Monitor: `sysbus.usbfs StartUSBIPServer 3240` (speed: 1 low, 2 full, 3 high).
    public void StartUSBIPServer(int port = 3240, int speed = 2)
    {
        if(usbipServer != null)
        {
            this.WarningLog("USB/IP server already running");
            return;
        }
        usbipServer = new USBIPBridge(this, port, (uint)speed);
        lock(HostLock)
        {
            BusReset();
        }
        FirePendingNewPackets();
    }

    public void StopUSBIPServer()
    {
        usbipServer?.Stop();
        usbipServer = null;
    }

    /// USB bus reset as the host drives it: packets in flight are lost.
    private void BusReset()
    {
        foreach(var ep in endpointsIn)
        {
            ep.Flush();
        }
        foreach(var ep in endpointsOut)
        {
            ep.DropHeld();
        }
        rxFifoPackets.Clear();
        ep0SetupsQueued = 0;
        EnqueueHostReset();
    }

    private bool BusResetPending => resetInterruptFlag.Value || hostResetPending;

    private USBIPBridge usbipServer;

    private class USBIPBridge
    {
        public USBIPBridge(DWC2_OTG dev, int port, uint speed)
        {
            this.dev = dev;
            this.speed = speed;
            listener = new TcpListener(IPAddress.Loopback, port);
            listener.Server.SetSocketOption(SocketOptionLevel.Socket, SocketOptionName.ReuseAddress, true);
            listener.Start();
            var t = new Thread(AcceptLoop) { IsBackground = true, Name = "usbip-accept" };
            t.Start();
            dev.InfoLog("USB/IP server on 127.0.0.1:{0} (busid 1-0)", port);
        }

        public void Stop()
        {
            stopped = true;
            try { listener.Stop(); } catch(Exception) { }
            session?.Close();
        }

        private void AcceptLoop()
        {
            while(!stopped)
            {
                TcpClient c;
                try
                {
                    c = listener.AcceptTcpClient();
                }
                catch(Exception)
                {
                    if(stopped)
                    {
                        return;
                    }
                    continue;
                }
                c.NoDelay = true;
                var t = new Thread(() => HandleClient(c)) { IsBackground = true, Name = "usbip-conn" };
                t.Start();
            }
        }

        // ---- OP_REQ_DEVLIST / OP_REQ_IMPORT ----

        private void HandleClient(TcpClient c)
        {
            var s = c.GetStream();
            try
            {
                var hdr = ReadExact(s, 8);
                var code = BE16(hdr, 2);
                if(code == OpReqDevlist)
                {
                    var d = DeviceRecord(true);
                    var r = new List<byte>();
                    PutBE16(r, Version); PutBE16(r, OpRepDevlist); PutBE32(r, 0);
                    PutBE32(r, d == null ? 0u : 1u);
                    if(d != null)
                    {
                        r.AddRange(d);
                    }
                    s.Write(r.ToArray(), 0, r.Count);
                    s.Flush();
                    c.Close();
                    return;
                }
                if(code != OpReqImport)
                {
                    dev.WarningLog("USB/IP: unexpected op 0x{0:X4}", code);
                    c.Close();
                    return;
                }
                var busid = System.Text.Encoding.ASCII.GetString(ReadExact(s, 32)).TrimEnd('\0');
                byte[] rec = null;
                var ok = busid == BusId && session == null;
                if(ok)
                {
                    // Attach = bus reset: the firmware re-enumerates; the host
                    // side waits until it handled the reset before the first URB.
                    lock(dev.HostLock)
                    {
                        dev.BusReset();
                    }
                    var t0 = Environment.TickCount;
                    while(Environment.TickCount - t0 < 2000)
                    {
                        lock(dev.HostLock)
                        {
                            if(!dev.BusResetPending)
                            {
                                break;
                            }
                        }
                        Thread.Sleep(2);
                    }
                    Thread.Sleep(20);
                    // vhci-hcd handles SET_ADDRESS itself and never sends it
                    // (a usbip server exports a device its own host already
                    // addressed): address it here, as that host would. The
                    // stock's GD32 library STALLs SET_CONFIGURATION in the
                    // Default state (usb-device in opendm40 accepts it).
                    if(ControlSync(new byte[] { 0x00, 5, 1, 0, 0, 0, 0, 0 }, null, 3000) == null)
                    {
                        dev.WarningLog("USB/IP: SET_ADDRESS after the bus reset failed");
                    }
                    rec = DeviceRecord(false);
                    ok = rec != null;
                }
                if(!ok)
                {
                    dev.WarningLog("USB/IP: import of '{0}' refused ({1})", busid, session != null ? "already imported" : "no such device / no descriptor");
                }
                var rep = new List<byte>();
                PutBE16(rep, Version); PutBE16(rep, OpRepImport); PutBE32(rep, ok ? 0u : 1u);
                if(ok)
                {
                    rep.AddRange(rec);
                }
                s.Write(rep.ToArray(), 0, rep.Count);
                s.Flush();
                if(!ok)
                {
                    c.Close();
                    return;
                }
                dev.InfoLog("USB/IP: device 1-0 imported");
                var sess = new Session(this, c);
                session = sess;
                sess.Run();     // returns when the connection closes
                session = null;
                dev.InfoLog("USB/IP: device 1-0 released");
            }
            catch(Exception e)
            {
                dev.DebugLog("USB/IP connection: {0}", e.Message);
                try { c.Close(); } catch(Exception) { }
            }
        }

        /// 312-byte usbip_usb_device (+ 4 bytes per interface when `withIfaces`).
        /// Read from the device over EP0 unless a session holds it (cache).
        private byte[] DeviceRecord(bool withIfaces)
        {
            if(session == null || devDesc == null)
            {
                var dd = ControlSync(new byte[] { 0x80, 6, 0, 1, 0, 0, 18, 0 }, null, 3000);
                if(dd == null || dd.Length < 18)
                {
                    return null;
                }
                var c9 = ControlSync(new byte[] { 0x80, 6, 0, 2, 0, 0, 9, 0 }, null, 3000);
                if(c9 == null || c9.Length < 9)
                {
                    return null;
                }
                var total = c9[2] | (c9[3] << 8);
                var cfg = ControlSync(new byte[] { 0x80, 6, 0, 2, 0, 0, (byte)total, (byte)(total >> 8) }, null, 3000);
                if(cfg == null || cfg.Length < 9)
                {
                    return null;
                }
                devDesc = dd;
                cfgDesc = cfg;
            }
            var r = new List<byte>();
            var path = new byte[256];
            var p = System.Text.Encoding.ASCII.GetBytes("/renode/virtual/" + BusId);
            Array.Copy(p, path, p.Length);
            r.AddRange(path);
            var b = new byte[32];
            var bb = System.Text.Encoding.ASCII.GetBytes(BusId);
            Array.Copy(bb, b, bb.Length);
            r.AddRange(b);
            PutBE32(r, 1);              // busnum
            PutBE32(r, 0);              // devnum
            PutBE32(r, speed);
            PutBE16(r, (ushort)(devDesc[8] | (devDesc[9] << 8)));   // idVendor
            PutBE16(r, (ushort)(devDesc[10] | (devDesc[11] << 8))); // idProduct
            PutBE16(r, (ushort)(devDesc[12] | (devDesc[13] << 8))); // bcdDevice
            r.Add(devDesc[4]); r.Add(devDesc[5]); r.Add(devDesc[6]);
            r.Add(cfgDesc[5]);          // bConfigurationValue
            r.Add(devDesc[17]);         // bNumConfigurations
            var ifaces = new List<byte[]>();
            for(var off = 0; off + 2 <= cfgDesc.Length && cfgDesc[off] > 0; off += cfgDesc[off])
            {
                if(cfgDesc[off + 1] == 4 && off + 8 <= cfgDesc.Length && cfgDesc[off + 3] == 0)
                {
                    ifaces.Add(new byte[] { cfgDesc[off + 5], cfgDesc[off + 6], cfgDesc[off + 7], 0 });
                }
            }
            r.Add((byte)ifaces.Count);
            if(withIfaces)
            {
                foreach(var i in ifaces)
                {
                    r.AddRange(i);
                }
            }
            return r.ToArray();
        }

        /// One control transfer outside a session (descriptors for DEVLIST /
        /// IMPORT). Null on timeout / STALL.
        private byte[] ControlSync(byte[] setup, byte[] outData, int timeoutMs)
        {
            var urb = new Urb { Setup = setup, Ep = 0, In = (setup[0] & 0x80) != 0, Length = (uint)(setup[6] | (setup[7] << 8)), OutData = outData };
            var deadline = Environment.TickCount + timeoutMs;
            var sig = new AutoResetEvent(false);
            Action h = () => sig.Set();
            dev.endpointsIn[0].NewPacket += h;
            try
            {
                lock(ep0Gate)
                {
                    var st = DoControl(urb, sig, () => Environment.TickCount - deadline > 0, out var data);
                    return st == 0 ? (data ?? new byte[0]) : null;
                }
            }
            finally
            {
                dev.endpointsIn[0].NewPacket -= h;
            }
        }

        // ---- transfers (run on the endpoint workers) ----

        /// Status 0 / -EPIPE / -ECONNRESET (cancelled) / -EOVERFLOW / -EREMOTEIO.
        private int DoControl(Urb u, AutoResetEvent sig, Func<bool> cancelled, out byte[] data)
        {
            data = null;
            var epIn = dev.endpointsIn[0];
            var epOut = dev.endpointsOut[0];
            // Leftovers of an abandoned (unlinked) control transfer.
            while(epIn.TryRead(out var _))
            {
            }
            var setup = Packet.Decode<SetupPacket>(u.Setup);
            var wLength = u.Setup[6] | (u.Setup[7] << 8);
            epOut.SetupPacketWrite(setup);
            if((u.Setup[0] & 0x80) != 0 && wLength > 0)
            {
                var st = ReadIn(epIn, Math.Min(wLength, (int)u.Length), sig, cancelled, out data);
                if(st != 0 && st != EOverflow)
                {
                    return st;
                }
                epOut.Write(new byte[0]);     // status stage: OUT ZLP
                return st;
            }
            if(wLength > 0 && u.OutData != null && u.OutData.Length > 0)
            {
                epOut.Write(u.OutData);
            }
            // status stage: IN ZLP from the device
            var s2 = ReadIn(epIn, 0, sig, cancelled, out var _);
            data = null;
            return s2;
        }

        /// Collect IN packets until `want` bytes or a short packet.
        private int ReadIn(EndpointIn ep, int want, AutoResetEvent sig, Func<bool> cancelled, out byte[] data)
        {
            var buf = new List<byte>();
            data = null;
            var mps = Math.Max(1, ep.MaxPacket);
            while(true)
            {
                if(cancelled())
                {
                    return EConnReset;
                }
                if(StallVisible(ep))
                {
                    return EPipe;
                }
                if(ep.TryRead(out var chunk))
                {
                    buf.AddRange(chunk);
                    if(chunk.Length == 0 || chunk.Length % mps != 0 || buf.Count >= want)
                    {
                        if(buf.Count > want)
                        {
                            data = buf.GetRange(0, want).ToArray();
                            return EOverflow;
                        }
                        data = buf.ToArray();
                        return 0;
                    }
                    continue;
                }
                sig.WaitOne(10);
            }
        }

        /// An EP0 STALL counts only once the driver took the current SETUP.
        private bool StallVisible(Endpoint ep)
        {
            lock(dev.HostLock)
            {
                return ep.Stalled && !(ep.Number == 0 && dev.ep0SetupsQueued > 0);
            }
        }

        private class Urb
        {
            public uint Seq;
            public uint DevId;
            public uint Ep;
            public bool In;
            public uint Flags;
            public uint Length;
            public uint NumPackets;
            public byte[] Setup;
            public byte[] OutData;
            public bool Done;
            public bool Cancelled;
        }

        // ---- an imported device: one TCP connection ----

        private class Session
        {
            public Session(USBIPBridge srv, TcpClient c)
            {
                this.srv = srv;
                this.c = c;
                stream = c.GetStream();
            }

            public void Close()
            {
                closed = true;
                try { c.Close(); } catch(Exception) { }
            }

            public void Run()
            {
                try
                {
                    while(!closed)
                    {
                        var h = ReadExact(stream, 48);
                        var cmd = BE32(h, 0);
                        var seq = BE32(h, 4);
                        if(cmd == CmdSubmit)
                        {
                            var u = new Urb
                            {
                                Seq = seq,
                                DevId = BE32(h, 8),
                                In = BE32(h, 12) == 1,
                                Ep = BE32(h, 16),
                                Flags = BE32(h, 20),
                                Length = BE32(h, 24),
                                NumPackets = BE32(h, 32),
                                Setup = new byte[8],
                            };
                            Array.Copy(h, 40, u.Setup, 0, 8);
                            if(!u.In && u.Length > 0)
                            {
                                u.OutData = ReadExact(stream, (int)u.Length);
                            }
                            if(u.Ep == 0)
                            {
                                u.In = (u.Setup[0] & 0x80) != 0;
                            }
                            lock(pending)
                            {
                                pending[seq] = u;
                            }
                            WorkerFor(u).Enqueue(u);
                        }
                        else if(cmd == CmdUnlink)
                        {
                            Unlink(seq, BE32(h, 20));
                        }
                        else
                        {
                            srv.dev.WarningLog("USB/IP: unexpected command 0x{0:X}", cmd);
                            break;
                        }
                    }
                }
                catch(Exception e)
                {
                    srv.dev.DebugLog("USB/IP session: {0}", e.Message);
                }
                closed = true;
                lock(pending)
                {
                    foreach(var u in pending.Values)
                    {
                        lock(u)
                        {
                            u.Cancelled = true;
                        }
                    }
                    pending.Clear();
                }
                foreach(var w in workers.Values)
                {
                    w.Stop();
                }
                try { c.Close(); } catch(Exception) { }
            }

            private void Unlink(uint seq, uint target)
            {
                Urb u;
                lock(pending)
                {
                    pending.TryGetValue(target, out u);
                }
                var status = 0;
                if(u != null)
                {
                    lock(u)
                    {
                        if(!u.Done && !u.Cancelled)
                        {
                            u.Cancelled = true;
                            status = EConnReset;
                        }
                        lock(pending)
                        {
                            pending.Remove(target);
                        }
                        SendRetUnlink(seq, status);
                    }
                }
                else
                {
                    SendRetUnlink(seq, status);
                }
                srv.dev.DebugLog("USB/IP: unlink seq {0} -> {1}", target, status);
            }

            private Worker WorkerFor(Urb u)
            {
                var key = u.Ep == 0 ? 0u : (u.Ep | (u.In ? 0x80u : 0u));
                lock(workers)
                {
                    if(!workers.TryGetValue(key, out var w))
                    {
                        w = new Worker(this, key);
                        workers[key] = w;
                    }
                    return w;
                }
            }

            /// Runs on a worker: the transfer, then RET_SUBMIT unless unlinked.
            public void Process(Urb u, AutoResetEvent sig)
            {
                Func<bool> cancelled = () => { lock(u) { return u.Cancelled || closed; } };
                byte[] data = null;
                int status;
                var actual = 0u;
                var dev = srv.dev;
                if(u.Ep >= EndpointsNumber)
                {
                    status = EPipe;
                }
                else if(u.Ep == 0)
                {
                    lock(srv.ep0Gate)
                    {
                        status = srv.DoControl(u, sig, cancelled, out data);
                    }
                    actual = u.In ? (uint)(data?.Length ?? 0) : (status == 0 ? u.Length : 0);
                }
                else if(u.In)
                {
                    var ep = dev.endpointsIn[u.Ep];
                    status = srv.ReadIn(ep, (int)u.Length, sig, cancelled, out data);
                    actual = (uint)(data?.Length ?? 0);
                    if(status == 0 && actual < u.Length && (u.Flags & UrbShortNotOk) != 0)
                    {
                        status = ERemoteIO;
                    }
                }
                else
                {
                    var ep = dev.endpointsOut[u.Ep];
                    if(srv.StallVisible(ep))
                    {
                        status = EPipe;
                    }
                    else
                    {
                        var d = u.OutData ?? new byte[0];
                        ep.Write(d);
                        var mps = ep.MaxPacket;
                        if(d.Length > 0 && mps > 0 && d.Length % mps == 0 && (u.Flags & UrbZeroPacket) != 0)
                        {
                            ep.Write(new byte[0]);
                        }
                        status = 0;
                        actual = (uint)d.Length;
                    }
                }
                lock(u)
                {
                    if(u.Cancelled)
                    {
                        return;
                    }
                    u.Done = true;
                    SendRetSubmit(u, status, actual, u.In ? data : null);
                }
                lock(pending)
                {
                    pending.Remove(u.Seq);
                }
            }

            private void SendRetSubmit(Urb u, int status, uint actual, byte[] data)
            {
                var r = new List<byte>(48 + (data?.Length ?? 0));
                // devid / direction / ep echoed (the kernel ignores them;
                // usbtmc_stress.py reads the IN data by this direction).
                PutBE32(r, RetSubmit); PutBE32(r, u.Seq); PutBE32(r, u.DevId); PutBE32(r, u.In ? 1u : 0u); PutBE32(r, u.Ep);
                PutBE32(r, (uint)status); PutBE32(r, actual); PutBE32(r, 0); PutBE32(r, u.NumPackets); PutBE32(r, 0);
                PutBE32(r, 0); PutBE32(r, 0);
                if(data != null && actual > 0)
                {
                    r.AddRange(data);
                }
                Send(r.ToArray());
            }

            private void SendRetUnlink(uint seq, int status)
            {
                var r = new List<byte>(48);
                PutBE32(r, RetUnlink); PutBE32(r, seq); PutBE32(r, 0); PutBE32(r, 0); PutBE32(r, 0);
                PutBE32(r, (uint)status);
                for(var i = 0; i < 6; i++)
                {
                    PutBE32(r, 0);
                }
                Send(r.ToArray());
            }

            private void Send(byte[] b)
            {
                lock(sendLock)
                {
                    try
                    {
                        stream.Write(b, 0, b.Length);
                    }
                    catch(Exception)
                    {
                        Close();
                    }
                }
            }

            public bool Closed => closed;

            public readonly USBIPBridge srv;
            private volatile bool closed;
            private readonly TcpClient c;
            private readonly NetworkStream stream;
            private readonly object sendLock = new object();
            private readonly Dictionary<uint, Urb> pending = new Dictionary<uint, Urb>();
            private readonly Dictionary<uint, Worker> workers = new Dictionary<uint, Worker>();
        }

        /// One endpoint (EP0: both directions): URBs in submission order.
        private class Worker
        {
            public Worker(Session s, uint key)
            {
                session = s;
                var n = key & 0x7F;
                if(n < EndpointsNumber)
                {
                    epIn = s.srv.dev.endpointsIn[n];
                    epIn.NewPacket += OnPacket;
                }
                thread = new Thread(Loop) { IsBackground = true, Name = "usbip-ep" + key.ToString("x2") };
                thread.Start();
            }

            public void Enqueue(Urb u)
            {
                lock(queue)
                {
                    queue.Enqueue(u);
                }
                work.Set();
            }

            public void Stop()
            {
                stop = true;
                work.Set();
                sig.Set();
            }

            private void OnPacket() => sig.Set();

            private void Loop()
            {
                try
                {
                    while(!stop && !session.Closed)
                    {
                        Urb u = null;
                        lock(queue)
                        {
                            if(queue.Count > 0)
                            {
                                u = queue.Dequeue();
                            }
                        }
                        if(u == null)
                        {
                            work.WaitOne(100);
                            continue;
                        }
                        lock(u)
                        {
                            if(u.Cancelled)
                            {
                                continue;
                            }
                        }
                        session.Process(u, sig);
                    }
                }
                catch(Exception e)
                {
                    session.srv.dev.WarningLog("USB/IP worker: {0}", e);
                }
                finally
                {
                    if(epIn != null)
                    {
                        epIn.NewPacket -= OnPacket;
                    }
                }
            }

            private volatile bool stop;
            private readonly Session session;
            private readonly EndpointIn epIn;
            private readonly Thread thread;
            private readonly Queue<Urb> queue = new Queue<Urb>();
            private readonly AutoResetEvent work = new AutoResetEvent(false);
            private readonly AutoResetEvent sig = new AutoResetEvent(false);
        }

        // ---- wire helpers ----

        private static byte[] ReadExact(NetworkStream s, int n)
        {
            var b = new byte[n];
            var off = 0;
            while(off < n)
            {
                var k = s.Read(b, off, n - off);
                if(k <= 0)
                {
                    throw new System.IO.EndOfStreamException("connection closed");
                }
                off += k;
            }
            return b;
        }

        private static ushort BE16(byte[] b, int o) => (ushort)((b[o] << 8) | b[o + 1]);

        private static uint BE32(byte[] b, int o) => ((uint)b[o] << 24) | ((uint)b[o + 1] << 16) | ((uint)b[o + 2] << 8) | b[o + 3];

        private static void PutBE16(List<byte> l, ushort v)
        {
            l.Add((byte)(v >> 8)); l.Add((byte)v);
        }

        private static void PutBE32(List<byte> l, uint v)
        {
            l.Add((byte)(v >> 24)); l.Add((byte)(v >> 16)); l.Add((byte)(v >> 8)); l.Add((byte)v);
        }

        private volatile Session session;
        private volatile bool stopped;
        private byte[] devDesc;
        private byte[] cfgDesc;
        private readonly object ep0Gate = new object();
        private readonly DWC2_OTG dev;
        private readonly uint speed;
        private readonly TcpListener listener;

        private const string BusId = "1-0";
        private const ushort Version = 0x0111;
        private const ushort OpReqDevlist = 0x8005;
        private const ushort OpRepDevlist = 0x0005;
        private const ushort OpReqImport = 0x8003;
        private const ushort OpRepImport = 0x0003;
        private const uint CmdSubmit = 1;
        private const uint CmdUnlink = 2;
        private const uint RetSubmit = 3;
        private const uint RetUnlink = 4;
        private const uint UrbShortNotOk = 0x0001;
        private const uint UrbZeroPacket = 0x0040;
        private const int EPipe = -32;
        private const int EConnReset = -104;
        private const int EOverflow = -75;
        private const int ERemoteIO = -121;
    }
}
}
