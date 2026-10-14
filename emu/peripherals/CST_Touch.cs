//
// Hynitron CST2xx/3xx-class touch controller at 7-bit address 0x5A as the
// DM40 uses it (firmware/re/analysis/touch_v2_6.h): 16-bit big-endian register
// addresses, D1 01 / D1 09 info mode, D2 04 chip info ([2] = 0xA8), D0 00
// 7-byte frame (point 0 + count + 0xAB), D0 07/0C/11/16 further points,
// D0 00 AB acknowledge. INT (PE5) pulses low for every report.
// Injection: Tap(x, y), Press(x, y), Move(x, y), Release(), Drag(x0,y0,x1,y1,ms),
// Key(code) for the side strip (MENU RUN AUTO MF = 0..3, reported as X beyond
// the 480 px screen). Coordinates are the 480x320 landscape frame.
//
using System;
using System.Collections.Generic;
using Antmicro.Renode.Core;
using Antmicro.Renode.Logging;
using Antmicro.Renode.Peripherals.I2C;
using Antmicro.Renode.Time;

namespace Antmicro.Renode.Peripherals.I2C
{
    public class CST_Touch : II2CPeripheral
    {
        public CST_Touch(IMachine machine)
        {
            this.machine = machine;
            IRQ = new GPIO();
            Reset();
        }

        public void Reset()
        {
            Array.Clear(mem, 0, mem.Length);
            reg = 0;
            wptr = 0;
            IRQ.Set(true); // idle high (pull-up)
            pressed = false;
        }

        public GPIO IRQ { get; }

        // ---- I2C ----
        public void Write(byte[] data)
        {
            foreach(var b in data)
            {
                if(wptr == 0) { reg = (ushort)(b << 8); wptr = 1; }
                else if(wptr == 1) { reg |= b; wptr = 2; }
                else { mem[reg & 0x3FF] = b; reg++; wptr++; }
            }
            if(wptr >= 2)
            {
                HandleCommand();
            }
            this.Log(LogLevel.Debug, "write {0} (reg 0x{1:X4} wptr {2})", BitConverter.ToString(data), reg, wptr);
        }

        public byte[] Read(int count = 1)
        {
            this.Log(LogLevel.Debug, "read {0} from 0x{1:X4}", count, reg);
            var r = new byte[count];
            for(var i = 0; i < count; i++)
            {
                r[i] = mem[reg & 0x3FF];
                reg++;
            }
            return r;
        }

        public void FinishTransmission()
        {
            wptr = 0;
        }

        private void HandleCommand()
        {
            switch(reg - (wptr - 2))
            {
            case 0xD101: infoMode = true; break;
            case 0xD109: infoMode = false; break;
            }
            if(wptr == 2)
            {
                // Register address only: prepare the read window.
                var r = reg;
                switch(r)
                {
                case 0xD204:
                    mem[0xD204 & 0x3FF] = 0x00; mem[0xD205 & 0x3FF] = 0x00;
                    mem[0xD206 & 0x3FF] = 0xA8; mem[0xD207 & 0x3FF] = 0x00;
                    break;
                }
            }
            else if(reg - 1 == 0xD002 && mem[0xD002 & 0x3FF] == 0xAB)
            {
                // D0 00 AB: frame acknowledged
                Acks++;
            }
        }

        // ---- reports ----
        private void Report(bool down, int x, int y, bool key)
        {
            ReportPoints(down ? new[] { x, y } : new int[0], key, x, y);
        }

        /// One frame: pts = x0,y0,x1,y1,... (pressed points, max 5); an empty list
        /// is a release report at (rx, ry). Record 0 at D000, records 1..4 at
        /// D007 + 5*(k-1); frame byte 5 = point count, byte 6 = 0xAB.
        private void ReportPoints(int[] pts, bool key, int rx = 0, int ry = 0)
        {
            var n = Math.Min(5, pts.Length / 2);
            var b = 0xD000 & 0x3FF;
            for(var k = 0; k < Math.Max(1, n); k++)
            {
                var x = n > 0 ? pts[2 * k] : rx;
                var y = n > 0 ? pts[2 * k + 1] : ry;
                var o = k == 0 ? b : (0xD007 + 5 * (k - 1)) & 0x3FF;
                mem[o + 0] = (byte)((key && k == 0 ? 0x80 : 0x00) | (k << 4) | (n > 0 ? 0x06 : 0x00));
                mem[o + 1] = (byte)((x >> 4) & 0xFF);
                mem[o + 2] = (byte)((y >> 4) & 0xFF);
                mem[o + 3] = (byte)(((x & 0xF) << 4) | (y & 0xF));
                mem[o + 4] = 0x10;
            }
            mem[b + 5] = (byte)Math.Max(1, n);
            mem[b + 6] = 0xAB;
            Reports++;
            this.Log(LogLevel.Debug, "report n={0} key={1} first=({2},{3})", n, key, n > 0 ? pts[0] : rx, n > 0 ? pts[1] : ry);
            // INT falling edge, released after 200 us
            IRQ.Set(false);
            machine.ScheduleAction(TimeInterval.FromMicroseconds(200), _ => IRQ.Set(true));
        }

        public void Press(int x, int y)
        {
            pressed = true; px = x; py = y;
            Report(true, x, y, false);
        }

        public void Move(int x, int y)
        {
            px = x; py = y;
            Report(true, x, y, false);
        }

        public void Release()
        {
            pressed = false;
            Report(false, px, py, false);
        }

        /// Multi-touch state "x,y;x,y;..." (all fingers currently down); "" releases.
        public void Touch(string points)
        {
            var list = new List<int>();
            foreach(var p in points.Split(new[] { ';' }, StringSplitOptions.RemoveEmptyEntries))
            {
                var xy = p.Split(',');
                list.Add(int.Parse(xy[0]));
                list.Add(int.Parse(xy[1]));
            }
            if(list.Count >= 2)
            {
                px = list[0]; py = list[1];
            }
            pressed = list.Count > 0;
            ReportPoints(list.ToArray(), false, px, py);
        }

        /// Press at (x, y) and release 120 ms later.
        public void Tap(int x, int y)
        {
            Press(x, y);
            machine.ScheduleAction(TimeInterval.FromMilliseconds(120), _ => Release());
        }

        /// Press at (x0,y0), move in 20 ms steps to (x1,y1) over ms, release.
        public void Drag(int x0, int y0, int x1, int y1, int ms)
        {
            Press(x0, y0);
            var steps = Math.Max(1, ms / 20);
            for(var i = 1; i <= steps; i++)
            {
                var k = i;
                machine.ScheduleAction(TimeInterval.FromMilliseconds((ulong)(20 * k)), _ =>
                    Move(x0 + (x1 - x0) * k / steps, y0 + (y1 - y0) * k / steps));
            }
            machine.ScheduleAction(TimeInterval.FromMilliseconds((ulong)(20 * steps + 60)), _ => Release());
        }

        /// `fingers` points 40 px apart around x, all moving by (dx, dy) over ms, then release
        /// (2- and 3-finger swipe-down: power-off / screenshot gestures).
        public void MultiSwipe(int fingers, int x, int y, int dx, int dy, int ms)
        {
            fingers = Math.Max(1, Math.Min(5, fingers));
            var steps = Math.Max(1, ms / 20);
            for(var i = 0; i <= steps; i++)
            {
                var k = i;
                Action a = () =>
                {
                    var pts = new int[2 * fingers];
                    for(var f = 0; f < fingers; f++)
                    {
                        pts[2 * f] = x + (f - (fingers - 1) / 2) * 40 + dx * k / steps;
                        pts[2 * f + 1] = y + dy * k / steps;
                    }
                    px = pts[0]; py = pts[1];
                    ReportPoints(pts, false);
                };
                if(k == 0) a();
                else machine.ScheduleAction(TimeInterval.FromMilliseconds((ulong)(20 * k)), _ => a());
            }
            machine.ScheduleAction(TimeInterval.FromMilliseconds((ulong)(20 * steps + 60)), _ => ReportPoints(new int[0], false, px, py));
        }

        /// Side key 0..3 (MENU RUN AUTO MF): key report with X < 500 / < 750 / < 1000 / >= 1000.
        public void Key(int code)
        {
            KeyDown(code);
            machine.ScheduleAction(TimeInterval.FromMilliseconds(150), _ => KeyUp());
        }

        public void KeyDown(int code)
        {
            keyX = KeyX[Math.Max(0, Math.Min(3, code))];
            ReportPoints(new[] { keyX, 100 }, true);
        }

        public void KeyUp()
        {
            ReportPoints(new int[0], false, keyX, 100);
        }

        public ulong Reports { get; private set; }
        public ulong Acks { get; private set; }

        private static readonly int[] KeyX = { 300, 600, 850, 1100 };
        private readonly IMachine machine;
        private readonly byte[] mem = new byte[0x400];
        private ushort reg;
        private int wptr;
        private bool infoMode, pressed;
        private int px, py, keyX = 300;
    }
}
