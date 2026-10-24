# DM40 emulator (Renode)

A full-system emulator of the Alientek DM40 (GD32F427VGT6, Cortex-M4F at
200 MHz) that runs the **stock firmware**, our **fw-rs native image** and the
complete **ROM -> FS-loader -> app boot chain** from the flash dump, renders the
ILI9488 LCD to PNG, accepts injected touches / side keys / the power key,
feeds scripted DMM and scope inputs, answers the Bluetooth module's AT setup,
and exposes the machine over GDB.

Everything lives under `emu/`. Nothing here touches the device.

```
emu/get-renode.sh      fetch the portable Renode build into emu/.renode/ (no sudo)
emu/run.py             driver: compose images, generate the .resc, run, screenshot
emu/dm40.repl          Renode platform description of the DM40
emu/peripherals/*.cs   DM40-specific device models (compiled by Renode at start-up)
emu/out/               generated images, scripts, logs, PNGs (git-ignored)
```

## Emulator choice: Renode

Owner's call, no QEMU comparison was run: Renode (portable Linux tarball, no
sudo, self-contained .NET runtime) because peripherals can be written as C# (or
Python) files compiled at start-up, it ships an NVIC/SysTick and an STM32F4
platform base (RCC, PWR, EXTI, timers, UART, SPI, flash controller, RTC, IWDG,
CRC), and it has a GDB server and a scriptable monitor. The STM32F4 models that
did not fit the DM40 were replaced (see "What is modelled").

Renode 1.17.0 is used (`emu/get-renode.sh`, about 150 MB, git-ignored).

## Quick start

```sh
emu/get-renode.sh                                   # once
emu/run.py stock --run-for 3 --shot emu/out/stock.png
emu/run.py stock --key menu@2.5 --tap 339,105@3.2 --scope sine,1000,1.0 \
           --run-for 5.5 --shot emu/out/stock-scope.png
emu/run.py fwrs  --run-for 4 --vin 1.234 --shot emu/out/fwrs.png
emu/run.py chain --trace-boot --run-for 4 --shot emu/out/chain.png
```

Modes:

| mode    | flash image                                                                 | starts at |
|---------|------------------------------------------------------------------------------|-----------|
| `stock` | dump + pristine V2.6 body (`dm40_v_2_6.dec.bin`) at 0x08004000, marker = header | app (0x08004000) |
| `fwrs`  | dump + fw-rs native body (`--app ELF|body|dm40*.bin`, default: build from a clean worktree of HEAD) | app |
| `chain` | as `stock` unless `--app` is given (`--app dump` keeps the dump's own app)     | ROM (0x08000000) |

The 1 MiB flash image is composed in `emu/out/flash-<name>.bin` from the
verified dump `firmware/re/dumps/full-20261013/flash_08000000_1MB.bin` (ROM,
FS-loader, UID-bound loader data at 0x080A4000, calibration at 0x080C0000); the
OTP (`otp_1FFF7800.bin`) and the chip UID
(`private/memrw-kit-test/full/uid_1FFF7A10.bin`) are loaded into system
memory at run time (the stock `SystemInit` only configures the 200 MHz PLL when
its UID check against 0x080A4000 passes; without the UID file the stock runs at
16 MHz). By default the UID's bytes 4 and 9 are replaced with the USB/IP port
(`run.py` `emu_uid`; the stock check never reads those two bytes), so each
instance has a chip UID, and with it a fw-rs USB / SCSI / `*IDN?` serial
number, of its own, never the real meter's: udisks merges disks with the same
vendor / model / serial into one drive. `--uid HEX24|FILE|unit` sets it
(`unit` = this unit's UID as is). The SPI-flash FAT comes from
`firmware/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin`, copied once to
`emu/out/disk.bin` (the working copy; the dump is never written).
None of these inputs are committed; generated images are git-ignored.

### Options (`emu/run.py --help`)

* `--flash IMG` use a 1 MiB image as-is; `--base IMG` another dump to compose from.
* `--model A|B|C` DM40 model: patches the calibration model byte (flash
  0x080C0168) and the record CRC in the composed image (with `--flash`: in a
  copy). Default: the dump's byte (this unit: DM40A). The stock then shows the
  4000/5000/6000-count range sets and AC+DC only on B/C; fw-rs reports it in
  `*IDN?` / `SYST:MOD?` (fw-rs/docs/model-variants.md). GTK panel: the DM40A/B/C
  dropdown in the header bar (restarts the emulator; also the printed label).
* `--disk IMG`, `--disk-add FILE[::NAME]` (mtools copy into a private copy of the
  disk), `--save-disk OUT`, `--save-flash OUT` (write the images back at the end).
* `--run-for S` virtual seconds (default 3). Events take `@T` (virtual seconds):
  * `--tap X,Y[@T]`, `--drag X0,Y0,X1,Y1[@T]` (300 ms), `--key menu|run|auto|mf|power|power-hold|pa2[@T]`
  * `--dmm SPEC[@T]` DMM input (HY3131 model; no spaces, several specs with `;`):
    `dcv:1.234`, `acv:1.0@50,dc=0.5,shape=square,duty=0.3`, `dca:0.5`,
    `aca:0.1@60`, `ohm:4700`, `short`, `open`, `diode:0.65`, `cap:100e-9`,
    `hz:1000,duty=0.25,amp=1.0`, `temp:100,cj=25` (full list and options in
    `emu/hy3131/README.md`); `--vin V[@T]` = DC voltage only (old scripts)
  * `--scope shape,Hz,Vpp[,offset][@T]` scope input (`sine square triangle ramp dc noise`)
  * `--shot PNG[@T]` (no `@T` = at the end), `--autoshot N` a PNG every N frames
  * `--script FILE` lines `T tap X Y | drag X0 Y0 X1 Y1 | key K | shot PNG | dmm SPEC | vin V | scope SPEC | monitor <renode command>`
* `--gdb [PORT]` GDB server (default 3333), `--pause` do not start the CPU,
  `--interactive` keep the Renode monitor open (`--dry` only writes the `.resc`).
* `--bt-port PORT` TCP passthrough of the Bluetooth UART (stock measurement
  protocol / fw-rs SCPI from the host). From scripts: `monitor sysbus.btmodule
  SetConnected true` raises PC14 ("module connected"), `SendText "..."` /
  `SendHex ...` inject host bytes (queued as a timed action, so they are safe
  from a script line while the emulation is paused and arrive after resume);
  the firmware's UART output is logged with `--debug sysbus.btmodule`.
* `--trace-boot` log every ROM / FS-loader / app region transition,
  `--pc-samples N` print PCs at the end, `--debug periph,...` Renode debug log
  for peripherals (e.g. `sysbus.i2c3,sysbus.i2c3.touch,sysbus.hy3131,sysbus.btmodule`).
* chain: `--pa2 T` releases the power key (PA2) at T (default 0.55 s: ROM ->
  FS-loader -> app; `0` = ROM launches the app directly; `-1` = never, the
  FS-loader stays in its install state with its USB disk), `--zero-marker`
  zeroes the loader marker so the FS-loader installs the first `dm40*.bin` on
  the FAT. The ROM's PA2 poll is a counted loop, so `--fast` (50 MIPS)
  stretches it 4x: use `--pa2 2.2` there (loader entered at 2.0 s), and give
  the install path ~20 s.

Coordinates are the 480x320 landscape frame the user sees. The stock main menu
tiles are at (46,52) Multimeter, (244,52) Oscilloscope, (46,166) Signal,
(244,166) Settings (190x106 each; `firmware/re/analysis/gui_v2_6.h`).

### Live window (`emu/live.py`)

```sh
/usr/bin/python3 emu/live.py stock --fast        # or fwrs | chain, plus any run.py option
```

A GTK4/libadwaita window drawn like the device (user manual figure 2.1.1 and
product photos): the display in its black glass with the MENU / RUN / AUTO /
MF touch strip on its left, the power button on the left edge, the V-Ohm /
COM / A jacks with their legends on the right, and above the front view a
top view of the top edge (USB-C with the charge LED, MCX OUT, MCX OSC). The
device sits top-left at the largest integer scale that leaves a 340 px card
column; the input cards fill the rest of the window: DMM jacks and the
front-end schematic on the right, scope input / generator+USB / monitor under
the device when they fit there (1440x900, the default), else at the end of
the right column (1280x800; it scrolls)
(Wayland-native, follows the system style; `--tk` or a missing PyGObject falls
back to a plain Tk window). The LCD is the ILI9488 model's framebuffer,
exported to a memory-mapped file 30 times a second and shown at an integer
device-pixel scale (nearest neighbour, HiDPI-aware); everything else goes over
Renode's monitor TCP port (`renode -P`), so the GUI never blocks the emulation.

* Display: mouse press / drag / release = touch; Ctrl+drag = two-finger pinch
  around the click point. Strip keys: press-and-hold (held = long press),
  pressed keys light up. Power button = PD6. Jacks / MCX / USB focus the
  matching input card (what is connected to the jacks -> the HY3131 model's
  `SetInput`, scope feeder waveform, generator/USB notes, a free monitor
  command line).
* Keyboard: `m r a f` = MENU RUN AUTO MF, `p` = power, `F2` / `F3` = 2-/3-finger
  swipe down (power-off confirm / screenshot gestures), `space` = pause/resume,
  `s` = screenshot to `emu/out/live-*.png`, `Esc` / Ctrl-C / SIGHUP / close =
  quit (Renode's process group is stopped, also when the window closes while
  Renode is still starting). A watchdog process next to Renode stops it when
  live.py dies without cleaning up (SIGKILL, crash), and a starting live.py
  stops leftover Renodes running `out/live.resc` that no live.py owns.
* Real-time cap (default on; header `≤1x`, `--no-realtime-cap` to start
  without it): Renode runs the guest as fast as the host allows, so a fast
  host or Fast mode put virtual time ahead of the wall clock; live.py pauses
  the emulation over the monitor while virtual time is ahead and resumes it
  when the clock has caught up (no catch-up of time lost while slower). Fast
  fw-rs DMM / scope runs: 0.97x average, <= 1.05x in any 1 s window (uncapped:
  up to 1.23x). `run.py` (batch) is not capped.
* Header bar: firmware (stock / fwrs / chain, restarts the machine), Fast
  (PerformanceInMips 50, roughly real time on this host), real-time cap, pause, restart,
  screenshot; the subtitle shows the virtual time and the speed ratio.
* Logs: `emu/out/live-renode.out` (Renode), `emu/out/live-monitor.log` (every
  monitor command and reply).

### USB over USB/IP

```sh
emu/live.py fwrs                                      # GUI: USB/IP on TCP 3240 by default (--no-usbip, --usbip-port P)
emu/run.py fwrs  --fast --usbip 0.01 --run-for 600     # headless: USB/IP on TCP 3240 (--usbip-port P) from t = 10 ms
                                                      # (the model's server; --usbip-renode: Renode's)
emu/run.py stock --app ../alientek-dm40-tools/patch/build/dm40_patched.bin --fast --usbip 6 --run-for 600
# other terminal (plain TCP, no root, no vhci):
emu/usbip_client.py list                 # busid=1-0 1209:0001 interfaces fe/03/01 08/06/50
emu/usbip_client.py desc   1-0           # device descriptor
emu/usbip_client.py config 1-0           # interfaces + endpoints
emu/usbip_client.py usbtmc 1-0 "*IDN?"   # -> Alientek,DM40A,<chip UID hex = emu_uid>,dm40-fw 0.1
emu/usbip_client.py scpi-tcp 1-0 5025    # SCPI lines on TCP 5025 <-> USBTMC, e.g. for
                                         # sigrok-cli -d scpi-dmm:conn=tcp-raw/127.0.0.1/5025
emu/usbip_client.py msc    1-0 0         # INQUIRY, READ CAPACITY, READ(10) LBA 0 (FAT boot sector); fw-rs native:
                                         # ONE LUN, the generated volume (4160541 x 512 B, src/vdisk.rs; read-only
                                         # by default, writable after Settings 3/3 USB disk = PC can edit + re-attach)
emu/usbip_client.py luns   1-0           # GET MAX LUN + per-LUN INQUIRY / TUR / capacity / MODE SENSE WP /
                                         # WRITE to the read-only LUN -> 07/27/00 / PREVENT-ALLOW
emu/usbip_client.py mscdump 1-0 0 out.img  # FAT volume of a LUN (metadata + used clusters, incl. the 16 MiB
                                         # DEBUG/SPIFLASH.BIN: ~2 min) for fsck.fat / mtools; resumable
                                         # (exit 2: restart the emulator, rerun)
emu/usbip_client.py upload 1-0 dm40-fw-native.bin  # fw-rs DBG:UPLOAD -> DM40FW.BIN in the SPI root (~8 s)
USBIP_PORT=3251 emu/usbip_client.py ...  # a server on another port (--usbip-port 3251)
emu/usbip_client.py read   1-0 0x1FFF7A10 12    # usbrw image: vendor EP0 read (the UID)
emu/usbip_client.py write  1-0 0x20028FC0 02    # usbrw image: vendor EP0 write + read-back
```

The USB/IP server is the model's own (`sysbus.usbfs StartUSBIPServer <port>`,
`emu/peripherals/DWC2_OTG_USBIP.cs`); `run.py --usbip-renode` falls back to
Renode's (`emulation CreateUSBIPServer` + `host.usb Register`), which only
`usbip_client.py` can use. The device is busid `1-0` (devid 0x10000), full
speed. Verified 2026-10-14 with Renode's server (not re-run on the model's:
its control OUT with a data stage is untested): the usbrw image (ep0 memrw
session) passes its acceptance (descriptor 28e9:02f0, UID read, RAM write +
read-back). fw-rs enumerates as 1209:0001, answers SCPI over USBTMC and serves the SPI disk over
MSC. Stock firmware: the device exists only after Settings > Other > USB
Connect; a DEVLIST / IMPORT whose descriptor read gets no answer within 3 s
lists no device / refuses the import. After USB Connect the stock (V2.6,
28e9:028f, one MSC interface: the 16 MiB SPI disk, 4096 x 4 KiB, writable)
attaches to the kernel and mounts (2026-10-22: `udisksctl mount -o ro`,
S000.BMP identical to the working image's, ~100 kB/s):

```sh
emu/run.py stock --fast --key menu@2.5 --tap 365,207@3.5 --tap 112,211@4.5 \
    --tap 309,65@5.5 --tap 219,96@6.5 --usbip 7 --run-for 1800    # Settings > Other > USB Connect
```

The desktop may automount it read-write (it is the working copy
`emu/out/disk.bin`). The stock never answers GET_DESCRIPTOR(string 4) (its
interface string pointer reads unmapped 0x00400000 in the emulator): the
kernel times out twice (2 x 5 s, unlinked) and goes on.

#### Attaching to the Linux kernel (vhci-hcd)

```sh
emu/run.py fwrs --fast --usbip 0.01 --usbip-port 3240 --run-for 3600 &   # usbip attach only knows TCP 3240
sudo modprobe vhci-hcd
sudo usbip attach -r 127.0.0.1 -b 1-0       # lsusb: a second 1209:0001, on the vhci bus (9 here)
ls /sys/bus/usb/devices/9-1:1.0/usbmisc/    # its /dev/usbtmcN (the real DM40 has another one)
DM40_USBTMC=emu fw-rs/tools/dm40scpi.py info    # emu = the usbtmc under vhci_hcd, real = the other,
DM40_USBTMC=emu fw-rs/tools/dm40dbg.py ping     # or DM40_USBTMC=/dev/usbtmcN
udisksctl mount -b /dev/sdX                 # sdX: the disk under vhci_hcd (/sys/block/sdX/device), mounts ro
sigrok-cli -d scpi-dmm:conn=usbtmc/9.N --samples 3   # libsigrok's usbtmc is libusb (bus.address);
                                            # scpi-dmm knows the IDN after SYST:IDN:EMUL 34401A
sudo usbip port; sudo usbip detach -p 0
```

Without root (`sudo` / the vhci sysfs `attach` are root-only):
`emu/kvmtest/run.sh <app.elf> <port> <LOG:DURation s> <log> [dm40direct=1]`
boots a KVM guest (the host's Ubuntu -generic kernel from its .deb, the
host's modules and usbip tool, busybox) that attaches the emulator over
vhci-hcd with `usbip --tcp-port`, mounts the volume (usb-storage, sd, vfat)
and drives SCPI over /dev/usbtmc0: a fast-log session, then INFO/ copies
while it streams (2026-10-23: the NOT READY answer of fw-rs vdisk.rs
`read_busy`; fw-rs/docs/native-boot.md "The slow flash bank while an AD2
stream runs"). Ports in 39000-39999 for parallel runs.

Verified 2026-10-22 (kernel 7.0): enumeration (device, BOS, strings,
config; the device qualifier STALLs -> -EPIPE as on hardware), usbtmc binds
and answers `*IDN?`, dm40scpi.py info / read, dm40dbg.py ping, usb-storage
+ udisks mount read-only (INFO/FIRMWARE.BIN byte-identical to the built
image, ~250 kB/s), sigrok scpi-dmm readings, a usbtmc read timeout (unknown
query) unlinked and the next query answered, detach + re-attach.

Why Renode's server could not do this (Renode 1.17.0, `USBIPServer.cs`):
the kernel's first request is GET_DESCRIPTOR(device, wLength 64) and Renode
completes a control IN only when wLength bytes arrived (`ReadAtLeast`), not on
the short packet; the 18-byte descriptor never filled it, the server thread
blocked for good, the kernel's UNLINK after 5 s was never read and the socket
died ~28 s later. Also missing there: RET_UNLINK, STALL (-EPIPE),
URB_SHORT_NOT_OK, one queue per endpoint (one pending Bulk-IN URB blocked
EP0 and everything else), full speed (it advertises high). The model's
server:

* per-endpoint workers (EP0 one worker for both directions), the socket
  reader never blocks; URBs of one endpoint complete in order;
* control IN ends on wLength or a short packet, then the OUT ZLP status;
  control OUT / no-data waits for the device's IN ZLP status; STALL (DxEPCTL
  bit 21, now modelled; EP0's is cleared by the next SETUP) -> -EPIPE;
* Bulk/Interrupt IN collects packets up to the URB length or a short packet;
  URB_SHORT_NOT_OK -> -EREMOTEIO (usb-storage's scatter-gather reads);
  Bulk OUT is split into max-packet packets, URB_ZERO_PACKET adds a ZLP;
* CMD_UNLINK -> RET_UNLINK -ECONNRESET for a pending URB (no RET_SUBMIT
  follows), 0 for one already completed (its RET_SUBMIT went out first);
* IMPORT is a USB bus reset (endpoint FIFOs dropped, the firmware
  re-enumerates; the reply waits until it handled USBRST) followed by
  SET_ADDRESS(1): vhci-hcd handles SET_ADDRESS itself and never sends it,
  and the stock's GD32 library STALLs SET_CONFIGURATION in the Default state
  (fw-rs's usb-device accepts it); one import at a time; DEVLIST of an
  imported device answers from cached descriptors.
* EP0 STALL is cleared when the driver pops the SETUP from the Rx FIFO
  (GRXSTSP), not when the host queues it, and the host ignores an EP0 STALL
  while a SETUP is still queued: the host here sends the next SETUP right
  after a status stage, before the driver handled it.
* IN endpoints give the host one packet per read (XFRSIZ counts down, TXFE
  when the FIFO is half empty and more is to come): the stock's 4 KiB MSC
  transfers do not fit the Tx FIFO and used to wait for the whole transfer.
* EP0 now always counts as active (USBAEP read 1 but its stored value was 0
  after a peripheral reset until the CPU next read DIEPCTL0).

Debugging the wire: `emu/usbip_proxy.py 3240 3241` (a transparent TCP proxy:
listens on 3240 for `usbip attach`, forwards to the emulator on
`--usbip-port 3241`) logs every SUBMIT / RET_SUBMIT / UNLINK; Renode's log has
`USB/IP: device 1-0 imported / released`.

Earlier fixes in the model (2026-10-21), still relevant: the USB/IP thread
cleared DIEPCTL.EPENA while the CPU polled that register, the clear was
lost and the IN endpoint looked busy forever: CPU register accesses and the
host side's endpoint calls are serialised (`DWC2_OTG.HostLock`); a Bulk-OUT
URB longer than 64 bytes is split into packets (a USBTMC message over 52
bytes used to be dropped). `usbtmc_stress.py` (thousands of command / query
pairs, multi-packet messages and replies, small TransferSize rounds,
pipelined URBs, stale REQUESTs, INITIATE_CLEAR, aborts; `--port`, `--seed`,
`--cases`) exercises fw-rs's USBTMC over USB/IP: 3000 iterations pass on the
model's server (144 s, 2026-10-23; MSC reads are no longer one main-loop turn per URB
round trip). Every reply must end with the `\n` terminator (848047f; a
block reply too, after the block). A Bulk-IN URB for which the device never sends data stays
pending on its own endpoint only (with Renode's server it blocked every
later URB).

Tests that start their own emulator take its USB/IP port from `--port`;
without it `mscresettest.py` and `ejecttest.py` pick a free one
(`run.free_port`: random in 61000-64999, above the kernel's ephemeral range
32768-60999 and outside the ranges handed to parallel runs, e.g. an
agent's 48000-48999; the port is printed). `DM40_EMU_PORTS=lo-hi` narrows
the choice to a range of one's own, e.g. `DM40_EMU_PORTS=48000-48999
emu/mscresettest.py --app ...`. (They defaulted to 4851 / 40200 before.)
`ejecttest.py` still needs TCP 3240 for its `usbip attach` proxy.

`gallerytest.py --app <ELF> [--port P] [--shots DIR]` drives the fw-rs
gallery (src/gallery.rs) like a user over USB/IP (DBG:KEY / TAP / SWIPE) on
a private copy of the MSC dump with 8 screenshots (S003..S007 added): grid
pages and swipes, the viewer (the whole screen read back with DBG:LCD? must
equal the BMP), Delete -> Cancel, Delete -> Delete (MMEM:CAT? 8 -> 7),
MENU back out (~8 min).

`wavtest.py --app <ELF> [--port P] [--bt-port P]` checks the scope waveform
readout (fw-rs src/scope_wav.rs) against the scope feeder's sine: the
default WAV:DATA?, WAV:POIN / FORM / PRE?, DIGitize / SINGle / TRIG:STAT?,
the 53 KB WORD record streamed over USBTMC in 4096-byte rounds and in one
64 KiB round, decimation, CHAN1:OFFS, an abandoned stream, and the
Bluetooth refusal (~5 min; USB/IP 24140, BT 24141). It turns the
comparator trigger off: the comparator path sometimes never completes a
frame after a restart in the emulator.

### GDB

```sh
emu/run.py stock --gdb 3333 --pause --interactive
# other terminal:
arm-none-eabi-gdb emu/out/fwrs-worktree/fw-rs/target/native/thumbv7em-none-eabihf/release/dm40-fw \
    -ex 'target remote :3333'
```

Memory, registers, breakpoints and single-stepping work as usual; `monitor`
commands go to the Renode monitor (`monitor sysbus.lcd Screenshot @/tmp/x.png`).

## Milestones (status 2026-10-13)

| | milestone | status |
|---|---|---|
| a | stock app boots to its main UI -> PNG | **reached**: DMM page at ~1 s virtual, 3 s run takes ~30 s wall |
| b | stock DMM and scope pages via injected taps -> PNG | **reached**: MENU key -> main menu -> Oscilloscope tile; scope page acquires and measures the fed waveform (`emu/out/stock-scope.png`) |
| c | fw-rs native image boots through the shim -> PNG | **reached**: DMM page with the scripted input, BT module set-up, menu, scope page (`emu/out/fwrs-scope.png`); USBTMC `*IDN?` and MSC over USB/IP |
| d | full chain ROM -> FS-loader -> app from the dump | **reached** (`--trace-boot`): ROM at 0, FS-loader at 0.50 s, app at 0.64 s; with PA2 held the loader shows its "open file error / Update" screen; with `--zero-marker --disk-add <update>` the loader installs the file into flash (verified byte-for-byte against the fw-rs body) and resets into it |

Check 5 (see "Use" below): `emu/out/stock-scope.png` and `emu/out/fwrs-scope.png`.

## What is modelled, what is stubbed

| block | model | notes |
|---|---|---|
| CPU, NVIC, SysTick | Renode `cortex-m4f`, SysTick at 200 MHz | PerformanceInMips 200 so busy loops roughly match virtual time |
| DWT CYCCNT | `DWT.cs` (sysbus.dwt) from virtual time | fw-rs's time base; every CYCCNT write is logged (the clock marker, "Input markers" below); `ZeroedAt` / `FwUptime` from the monitor |
| CPU cost | `CpuCost.cs` (sysbus.cpucost) | the device is ~1.8x slower per instruction than PerformanceInMips 200: `--mips 110` matches its idle AC V main-loop pass (22 vs 23 us) and its 76.8 kS/s disk Run pass (617-681 vs 718 us, ~1400 vs 1272 passes/s; fw-rs device check 7409b92). `IsrStallNs n`: n ns per stream ISR entry inside the busy window fw-rs measures (cpu_permille; 700 at --mips 110 gives ~520 against the device's 544 at 76 800 S/s: the device runs 1.48 ISR entries per sample there, the model 1.13); it lengthens every entry, so a large n adds overruns the device does not have. `PcSampleStart` / `PcSampleDump path`: the interrupted main-code PC at every ISR entry (a profile of the main loop while the stream runs). Default: off, --mips 200 |
| flash 1 MiB, SRAM 192 K, TCM 64 K, backup SRAM, system memory (OTP/UID) | mapped memory | option bytes via the flash controller's region |
| FMC flash controller | Renode `STM32F4_FlashController` | key/lock, sector erase (STM32F4 sector map), BUSY never set; programming = plain writes (the FS-loader's install and fw-rs's marker zeroing work) |
| RCC, PWR | Renode `STM32F4_RCC`, `STM32_PWR` | ready bits follow enable bits (HSE/PLL/PLLSAI), over-drive handshake modelled; clock *values* are not: every timer runs at 200 MHz (TIMPRE) |
| FSMC | registers = RAM; bank 1 = `ILI9488.cs` | A16 = RS; 2A/2B/2C/2E (RGB666 read-back packing), 36 MADCTL, 3A, D3 ID = 0x9488, 11/29; `Screenshot path`, `AutoShot dir n`, `DumpGram` |
| GPIO A..I | `GD32_GPIO.cs` | own model: Renode 1.17.0's `STM32_GPIOPort` writes every pin's input state on an ODR write (no mode check), which broke the bit-banged HY3131 MISO. Pull-ups give the idle level of undriven inputs (PA2 reads high unless driven). PE5 -> EXTI line 5, PA15 -> TIM2 CH1 |
| EXTI, SYSCFG | Renode `STM32F4_EXTI`; SYSCFG = RAM | only line 5 (touch INT) is wired |
| TIM1, TIM2, TIM5, TIM8 | `GD32_Timer.cs` | own model: counter from virtual time, slave mode controller (SMS reset / gated / trigger / external clock, TS = ITR0..3 wired per the GD32F4 interconnect table, TI1FP1 / TI1F_ED from the CH1 pin with CCER polarity), one-pulse, CC1 capture, OC PWM1/PWM2/force, TRGO (enable, update, compare pulse, OCxREF level), UIE/CCxIE/TIE on the general or the split advanced lines. Flags are evaluated lazily and interrupts scheduled at their exact virtual time, so a 50 MHz TIM8 costs nothing. No DMA request lines: the scope feeder asks TIM1 for its update events |
| TIM4, TIM7, TIM9..11 | Renode `STM32_Timer` at 200 MHz | interrupt tick timers |
| TIM3, TIM6, TIM12..14 | `PwmTimer.cs` | PWM / DAC trigger: registers, CNT from virtual time, no events |
| DMA1/DMA2 | `GD32_DMA.cs` | own model: one request = one data unit, NDTR counts down, HTIF/TCIF, circular, double buffer, M2M; `Feed(stream, value)` for the ADC feeder |
| SPI1 (0x40003800) + W25Q128 | `GD32_SPI.cs` (timed bytes) + `W25Q128.cs` | file-backed 16 MiB, CS = PE4; READ/FAST READ/PP/SE/BE/CE/WREN/RDSR/REMS/JEDEC/RUID; `Load`, `Save`; busy times `ProgramUs` / `Erase*Us` (fastlogtest profiles); `StrictBusy` (default): a command other than a status read during BUSY is ignored (a READ gives 0xFF), as on the chip. The bus's `IdleStartNs` (run.py `--spi-disk-gap-ns`, default 330): a byte written to an idle shifter starts that much later, so a polled write-wait-read byte costs ~0.97 us at 12.5 MHz as on the device (back-to-back page-program bytes 0.64 us); without it the INFO/ mirror's reads during a 76800 disk session looked ~20x cheaper than on the device (fw-rs TODO, device 612c45c M2) |
| I2C2 (0x40005C00) + touch | `GD32_I2C.cs` + `CST_Touch.cs` | own I2C master (SBSEND/ADDSEND/BTC/TBE/RBNE, AERR for absent addresses, ACKEN/POAP, DMA RX/TX requests, DMALST); Hynitron 0x5A protocol (D1 01/09, D2 04, D0 00 frame, D0 07.., D0 00 AB ack); `Tap`, `Press`, `Move`, `Release`, `Drag`, `Key` |
| HY3131 DMM ADC | `HY3131.cs` (a symlink to `emu/hy3131/HY3131.cs`, as is `DM40Frontend.cs`; that README has the research and tests) | full register map with the datasheet semantics (INTF cleared on read, CTA/CTB/CTC counter, ENxxx clears, R37 reset); conversion timing: AD1 period = OSR1 * 30 / SYSCLK, doubled with the chopper (25 / 100 / 400 ms), AD2/AD3 + LPF + true RMS at their own rates, frequency counter, comparator (CMP pin), peak hold, temperature sensor; a per-row analog model of every stock configuration (V, mV, A ranges, ohm, continuity, diode, capacitance by RC oscillator and current ramp, Hz/duty, thermocouple with cold junction) that applies the meter's own calibration so the stock and fw-rs show the scripted input. Inputs: `SetInput "spec"` / `--dmm SPEC[@T]` / script `T dmm SPEC`; `SetVin V` kept for old scripts |
| 3PA1030 scope ADC + comparator | `ScopeFeeder.cs` | sample n = TIM1 update event n (`GD32_Timer.UpdateEventsUntil/TimeOfUpdate`, so TIM5's trigger start, one-pulse stops and TIM2 gating are exact); code = 512 + V * counts/V of the front-end range read from PC13/PB15/PD12/PD13; samples go to DMA2 stream 4 in bulk every 100 us and before any stream register access (NDTR exact). Comparator: crossings found one tick ahead, PA15 driven at the crossing's virtual time + 2 us (the board's comparator latency = stock `trig_delay`), INVERTED (high while signal < level, as `fw-rs/src/hwtrig.rs` documents), level = DAC2 with the stock default map (1780 -> code 300, 2401 -> 780), 2 codes hysteresis |
| UART4 + BT module | Renode `STM32_UART` + `BtModule.cs` | AT+SBN/CBN/SLED0/RESET answered, PC14 "connected" while a TCP host is attached (`--bt-port`), `SendHex`/`SendText` from scripts |
| ADC0 | `SimpleADC.cs` | SW-started single conversions, GD32 calibration bits self-clear; channel 15 = 4.0 V battery by default (`SetVolts ch v`) |
| DAC | RAM | DHR12R2 is read by the scope feeder for the trigger level; no analog output |
| FWDGT (free watchdog) + RCU_RSTSCK reset flags | `FWDGT.cs` (sysbus.fwdgt) | own model (Renode 1.17.0's `STM32_IndependentWatchdog` never reset the machine here, and its RCC returned a constant RSTSCK): 0xCCCC / 0xAAAA / 0x5555, PSC / RLD at 32 kHz (stock PSC 6, RLD 2500 = 20 s); a timeout resets the machine like SYSRESETREQ (flash, backup SRAM, RAM keep their content). RSTSCK flags: BOR\|PIN\|POR after power-on, + FWDGT\|PIN after a timeout, + SW\|PIN after a SYSRESETREQ, cleared by RSTFC (run.py routes RSTSCK and AIRCR to the model with bus hooks). `Flags`, `Timeouts`, `PowerOn` from the monitor. A firmware that stops feeding now really reboots (the ROM already starts it; the stock feeds it from its 1 kHz timer ISR). Tested by `emu/recoverytest.py` |
| RTC, CRC, RNG | Renode models | |
| USB OTG FS (DWC2) | `DWC2_OTG*.cs` | Renode 1.17.0's `STM_USB` (the Synopsys OTG device model, renamed `DesignWare_USB_OTG` upstream) with the fixes the GD32 drivers need: DTXFSTS = programmed FIFO depth minus queued words (the stock driver only writes while free > needed), EP0's 2-bit MPSIZ code decoded (0 = 64 bytes), OUT data held while an endpoint NAKs. Exported to Renode's USB/IP server (see "USB over USB/IP"); IRQ 67 |
| buzzer (TIM14 PWM), backlight (TIM13 PWM), relays (PA9/PA10, PE0/PE1, PC13...), rails (PE2/PC4) | GPIO/timer registers only | nothing observes them yet |

Not modelled: the generator's analog output (DAC1 + TIM6 DMA runs, nothing
listens), PLL/bus clock values (always 200 MHz), MCO, low-power modes.

## Speed

Wall time for the two reference commands (32-core host, one Renode thread
each), `--stats`:

| command | before | after | after, `--fast` |
|---|---|---|---|
| `stock --run-for 5.5 ... --scope sine,1000,1.0` (stock-scope.png) | 22.1 s (4.0x real time) | 20.8 s (3.8x) | 8.8 s (1.6x) |
| `fwrs --run-for 6 ... --scope sine,1000,1.0` (fwrs-scope.png) | did not finish in 23 min (12.5 MS/s fed sample by sample) | 18.6 s (3.1x) | 11.2 s (1.9x) |
| `stock --run-for 3` (boot to the DMM page) | 23.7 s (7.9x) | 17.7 s (5.9x) | 10.7 s (3.6x) |

Measured with the final models (GD32_Timer for TIM1/2/5/8, sample-exact
feeder). What was done (`emu/run.py --stats` prints wall / virtual /
instructions):

1. Output-only timers (TIM1/8 sample clocks at 50 MHz, TIM6 at 10 MHz, the
   PWM timers) are an event-free model (`PwmTimer.cs`); Renode's
   `STM32_Timer` handled every overflow as an event. Screenshots are identical.
2. The scope feeder hands whole sample runs to the DMA model
   (`GD32_DMA.FeedBlock`, one bus write per chunk, split at comparator edges)
   instead of one request per sample.
3. The C# models are merged into one compile unit (`emu/out/dm40-models.cs`):
   Renode compiled each `include` separately (start-up 5.6 s -> 3.2 s).
4. `--fast` = `PerformanceInMips 50`. Both firmwares spin in their main loops,
   so the wall time is set by instructions per virtual second; at 50 MIPS the
   UI work takes up to 4x longer in virtual time (fw-rs needs ~4 s instead of
   ~2.5 s before it takes the first key), timing-sensitive code is unchanged
   (all delays are timer / DWT based). Screens differ only where the UI is
   mid-update.
5. Measured and rejected: the global quantum (no effect), the LCD data path
   (the ILI9488 write is already a plain array store; the GUI draws about
   150 k pixels per second, far below the instruction cost).

The remaining cost is the guest's busy loops: the stock idles in its GUI loop
at 200 MIPS = 600 M instructions per 3 s. Fast-forwarding known delay loops
would need per-firmware hooks and was not done.

## Known limitations / next steps

* Speed: see above; 2-4x slower than real time at full MIPS, about real time
  with `--fast`.
* fw-rs: taps and side keys work; horizontal swipes were not recognised by
  the gesture engine in the emulator (the drag reports arrive every 20 ms
  with an INT pulse each; not investigated).
* Scope trigger (verified 2026-10-14, `--fast`, screenshots in `emu/out/`):
  stock Auto with a 1 kHz sine at 10 ms/div -> `T'D`; 40 Hz square -> the
  falling edge sits on the T marker with the default Edge = Falling, the
  rising edge with Edge = Rising (`stock-trigger-square40-*.png`); AUTO-set
  of the sine lands at 5 us/div = 50 MS/s with a continuous trace through
  the marker (`stock-trigger-1khz-5us.png`); Normal mode -> `T'D` with the
  signal and `WAIT` after `--scope dc,0,0` (`stock-trigger-normal-*.png`).
  fw-rs: its comparator engine (TIM2 PWM2 OC2REF gating TIM1 from ITR1) runs
  in the emulator once enabled (`fwrs-hwtrig.png`); `DBG:*` is refused over
  Bluetooth, so until USB exists enable it by patching `HwTrig::available`
  to `movs r0,#1; bx lr` before start (script lines
  `0 monitor sysbus WriteWord 0x08025870 0x2001` / `0x08025872 0x4770`,
  address from `llvm-nm` of the ELF). Timebase/trigger controls of the
  stock: MF key -> popup, Trigger tab at (263,42), Rising (55,92) /
  Falling (115,92), Auto (245,92) / Normal (305,92) / Single (365,92).
* USB: EP0, USBTMC and MSC work over USB/IP with the forked DWC2 model;
  isochronous/interrupt endpoints, suspend/resume and the host-kernel
  attach path are untested.
* The ROM's and FS-loader's PA2 timing is reproduced only empirically
  (release at 0.55 s gives the three-stage chain).

## Use

Milestone 5: `emu/out/stock-scope.png` is the stock scope page in its default
state with a 1 kHz 1 Vpp sine on the input (10 ms/div -> a dense band, Freq
1.00 kHz, Vp-p 902 mV, Vrms 341 mV measured by the stock), and
`emu/out/fwrs-scope.png` is fw-rs's scope page with the same input. Both are
generated by:

```sh
emu/run.py stock --name scope --run-for 5.5 --key menu@2.5 --tap 339,105@3.2 \
    --scope sine,1000,1.0 --shot emu/out/stock-scope.png
emu/run.py fwrs --name fwrs-scope --run-for 6 --key menu@2.5 --tap 339,105@3.2 \
    --scope sine,1000,1.0 --shot emu/out/fwrs-scope.png
```

Other useful checks: `--vin 1.234` shows 1.2488 V on the stock (the dump's
calibration gain applied) and the raw-scaled value on fw-rs; the loader
install path (`chain --pa2 -1 --zero-marker --disk-add <dm40 update>`) exercises
the flash controller, the SPI disk and the reset path end to end.

Recovery triggers (fw-rs shim, `fw-rs/src/boot.rs`): `emu/recoverytest.py
--app <fw-rs ELF>` runs the chain ROM -> FS-loader -> fw-rs and checks the
power-key hold from off (short press, abort in the confirm window, the full
hold -> FS-loader update screen), the watchdog crash counter (one hang = a
reboot, a second hang within 60 s of running main loop = the update screen,
a second one after 60 s = a reboot) and, with `--cases release`, DBG:RELEASE
over USB. About 6 min wall for the default 8 cases at `--jobs 8`.

### Input markers (test timing)

Scenario tests change the input at fixed virtual times (`--script` /
`--dmm SPEC@T`) and read the firmware's uptime (DBG:PING?). The two clocks
differ by the boot: fw-rs's uptime is the virtual time since its last DWT
CYCCNT write (`Clock::new` in main), 3.56 s of virtual time on a fresh disk (7bd7f6f;
2.85 s on ab7184c before the device-like SPI byte time) with `--fast` (1.90 s on ab7184c at `--mips 200`; pre_main's SPI-disk
work: fatdir, calbackup, `vdisk::mirror_boot`).
The offset is the same in every run (virtual time is deterministic, host load
or not), but it grows whenever the boot does: it was 0.5, 0.9, 1.3, 1.8 s as
code was added, and tests carried those guesses (VLAG / LAG / BOOT_OFS /
"next phase - 4.5 s"). With 2.85 s, couptest's and powtest's check windows
reached 0.35 s past the next phase's input; whether a reading landed there
depended on where the main loop's reading cadence fell (the A AUTO coupling
reads in bursts every ~1.9 s), which looked like a bimodal offset under load
(couptest idc: the 0.3 A input "at fw 13.2 or 14.8 s"; it was at 13.15 s
both times, seen by this burst or the next).

The model logs both clocks instead, and the tests key on that:

    dwt: CYCCNT write 0x00000000 at 2.849631           (DWT.cs: the clock marker)
    hy3131: input: open;dca:0.3 at 16.0000 fw 13.1504  (HY3131 SetInput: every input change)

`emu/emusync.py` reads the run's Renode log as it is written (`Markers`:
`wait_clock()` after the USB attach, `fw(v)`, `input_fw(v)`, `at(v)`) and
places a scenario's phases in fw uptime (`Timeline`: a phase starts at its
input's marker, or at its script distance after the previous start when it
starts with an action; actions at their distance from their phase's start;
check windows from the next phase's start backwards). A run whose markers are
missing, or whose fw clock restarted (a reset), fails. Keyed this way:
couptest, powtest, senttest, burdentest, leadstest, acdcmethtest,
autowaittest, switchlat, inrushtest, dcvtest (the 25 V step), loudtest (its
actions on the script's clock), switchtest (`later` inputs land inside the
first step). Tests driven only by SCPI handshakes and uptime (autorangetest,
modetest, switchtest's steps) need nothing.

## Files the emulator reads (never writes)

* `firmware/re/dumps/full-20261013/flash_08000000_1MB.bin`, `otp_1FFF7800.bin`
* `private/memrw-kit-test/full/uid_1FFF7A10.bin` (bytes 4 / 9 = the USB/IP port; `--uid` to set it)
* `firmware/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin` (copied to `emu/out/disk.bin`)
* `dm40_v_2_6.dec.bin` (stock body), `firmware/dm40fw.py` (header words)
* fw-rs: `git worktree add --detach emu/out/fwrs-worktree HEAD` + `./build.sh native`
  (`native` = the default and only build now; kept so `--rev` of older commits builds)
  (left in place for reuse; `git worktree remove emu/out/fwrs-worktree` to drop it)
