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
16 MHz). The SPI-flash FAT comes from
`firmware/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin`, copied once to
`emu/out/disk.bin` (the working copy; the dump is never written).
None of these inputs are committed; generated images are git-ignored.

### Options (`emu/run.py --help`)

* `--flash IMG` use a 1 MiB image as-is; `--base IMG` another dump to compose from.
* `--disk IMG`, `--disk-add FILE[::NAME]` (mtools copy into a private copy of the
  disk), `--save-disk OUT`, `--save-flash OUT` (write the images back at the end).
* `--run-for S` virtual seconds (default 3). Events take `@T` (virtual seconds):
  * `--tap X,Y[@T]`, `--drag X0,Y0,X1,Y1[@T]` (300 ms), `--key menu|run|auto|mf|power|power-hold|pa2[@T]`
  * `--vin V[@T]` DMM input voltage (DC V ranges), `--scope shape,Hz,Vpp[,offset][@T]`
    scope input (`sine square triangle ramp dc noise`)
  * `--shot PNG[@T]` (no `@T` = at the end), `--autoshot N` a PNG every N frames
  * `--script FILE` lines `T tap X Y | drag X0 Y0 X1 Y1 | key K | shot PNG | vin V | scope SPEC | monitor <renode command>`
* `--gdb [PORT]` GDB server (default 3333), `--pause` do not start the CPU,
  `--interactive` keep the Renode monitor open (`--dry` only writes the `.resc`).
* `--bt-port PORT` TCP passthrough of the Bluetooth UART (stock measurement
  protocol / fw-rs SCPI from the host).
* `--trace-boot` log every ROM / FS-loader / app region transition,
  `--pc-samples N` print PCs at the end, `--debug periph,...` Renode debug log
  for peripherals (e.g. `sysbus.i2c3,sysbus.i2c3.touch,sysbus.hy3131,sysbus.btmodule`).
* chain: `--pa2 T` releases the power key (PA2) at T (default 0.55 s: ROM ->
  FS-loader -> app; `0` = ROM launches the app directly; `-1` = never, the
  FS-loader stays in its install state with its USB disk), `--zero-marker`
  zeroes the loader marker so the FS-loader installs the first `dm40*.bin` on
  the FAT.

Coordinates are the 480x320 landscape frame the user sees. The stock main menu
tiles are at (46,52) Multimeter, (244,52) Oscilloscope, (46,166) Signal,
(244,166) Settings (190x106 each; `firmware/re/analysis/gui_v2_6.h`).

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
| c | fw-rs native image boots through the shim -> PNG | **reached**: DMM page with the scripted input, BT module set-up, menu, scope page (`emu/out/fwrs-scope.png`) |
| d | full chain ROM -> FS-loader -> app from the dump | **reached** (`--trace-boot`): ROM at 0, FS-loader at 0.50 s, app at 0.64 s; with PA2 held the loader shows its "open file error / Update" screen; with `--zero-marker --disk-add <update>` the loader installs the file into flash (verified byte-for-byte against the fw-rs body) and resets into it |

Check 5 (see "Use" below): `emu/out/stock-scope.png` and `emu/out/fwrs-scope.png`.

## What is modelled, what is stubbed

| block | model | notes |
|---|---|---|
| CPU, NVIC, SysTick | Renode `cortex-m4f`, SysTick at 200 MHz | PerformanceInMips 200 so busy loops roughly match virtual time |
| DWT CYCCNT | `DM40_DWT.cs` from virtual time | fw-rs's time base |
| flash 1 MiB, SRAM 192 K, TCM 64 K, backup SRAM, system memory (OTP/UID) | mapped memory | option bytes via the flash controller's region |
| FMC flash controller | Renode `STM32F4_FlashController` | key/lock, sector erase (STM32F4 sector map), BUSY never set; programming = plain writes (the FS-loader's install and fw-rs's marker zeroing work) |
| RCC, PWR | Renode `STM32F4_RCC`, `STM32_PWR` | ready bits follow enable bits (HSE/PLL/PLLSAI), over-drive handshake modelled; clock *values* are not: every timer runs at 200 MHz (TIMPRE) |
| FSMC | registers = RAM; bank 1 = `ILI9488.cs` | A16 = RS; 2A/2B/2C/2E (RGB666 read-back packing), 36 MADCTL, 3A, D3 ID = 0x9488, 11/29; `Screenshot path`, `AutoShot dir n`, `DumpGram` |
| GPIO A..I | `GD32_GPIO.cs` | own model: Renode 1.17.0's `STM32_GPIOPort` writes every pin's input state on an ODR write (no mode check), which broke the bit-banged HY3131 MISO. Pull-ups give the idle level of undriven inputs (PA2 reads high unless driven). PE5 -> EXTI line 5, PA15 -> TIM2 CH1 |
| EXTI, SYSCFG | Renode `STM32F4_EXTI`; SYSCFG = RAM | only line 5 (touch INT) is wired |
| TIM1..14 | Renode `STM32_Timer` at 200 MHz | interrupts, one-pulse, PWM registers; no DMA requests, no ITRx slave start (the scope feeder works around both), no output pins |
| DMA1/DMA2 | `GD32_DMA.cs` | own model: one request = one data unit, NDTR counts down, HTIF/TCIF, circular, double buffer, M2M; `Feed(stream, value)` for the ADC feeder |
| SPI1 (0x40003800) + W25Q128 | Renode `STM32SPI` + `W25Q128.cs` | file-backed 16 MiB, CS = PE4; READ/FAST READ/PP/SE/BE/CE/WREN/RDSR/REMS/JEDEC/RUID; `Load`, `Save` |
| I2C2 (0x40005C00) + touch | `GD32_I2C.cs` + `CST_Touch.cs` | own I2C master (SBSEND/ADDSEND/BTC/TBE/RBNE, AERR for absent addresses, ACKEN/POAP, DMA RX/TX requests, DMALST); Hynitron 0x5A protocol (D1 01/09, D2 04, D0 00 frame, D0 07.., D0 00 AB ack); `Tap`, `Press`, `Move`, `Release`, `Drag`, `Key` |
| HY3131 DMM ADC | `HY3131.cs` on the bit-banged pins | register file, INTF always "all converters ready", AD1 from `SetVin` (stock DC V scale of the configured range row) or `SetAd1`; AC/RMS/LPF/counter values are placeholders |
| 3PA1030 scope ADC + comparator | `ScopeFeeder.cs` | delivers waveform samples to DMA2 stream 4 at the TIM1 rate (TIM1 running, or slave of a running TIM5 as the stock does), code = 512 + V * counts/V of the front-end range read from PC13/PB15/PD12/PD13; comparator output on PA15 from DAC2's level (stock default calibration) |
| UART4 + BT module | Renode `STM32_UART` + `BtModule.cs` | AT+SBN/CBN/SLED0/RESET answered, PC14 "connected" while a TCP host is attached (`--bt-port`), `SendHex`/`SendText` from scripts |
| ADC0 | `SimpleADC.cs` | SW-started single conversions, GD32 calibration bits self-clear; channel 15 = 4.0 V battery by default (`SetVolts ch v`) |
| DAC | RAM | DHR12R2 is read by the scope feeder for the trigger level; no analog output |
| IWDG, RTC, CRC, RNG | Renode models | the stock feeds the IWDG from its 1 kHz timer ISR |
| USB OTG FS | `USBFS_Stub.cs` | registers only: AHBIDL set, resets self-clear, no interrupts -> never enumerates. The FS-loader's and the stock's MSC bring-up run through; fw-rs's USBTMC never sees a host |
| buzzer (TIM14 PWM), backlight (TIM13 PWM), relays (PA9/PA10, PE0/PE1, PC13...), rails (PE2/PC4) | GPIO/timer registers only | nothing observes them yet |

Not modelled: the generator's analog output (DAC1 + TIM6 DMA runs, nothing
listens), PLL/bus clock values (always 200 MHz), MCO, low-power modes.

## Known limitations / next steps

* Speed: about 10x slower than real time while the UI is idle (a 3 s virtual
  run takes ~30 s). The scope at fast timebases is much slower (the feeder
  writes every sample through the DMA model).
* fw-rs: taps and side keys work; horizontal swipes were not recognised by
  the gesture engine in the emulator (the drag reports arrive every 20 ms
  with an INT pulse each; not investigated).
* HY3131: only the DC V path is scaled; other modes read the raw placeholder
  values. An AC/ohm/capacitance model needs the per-mode scaling from
  `firmware/re/analysis/dmm_v2_6.h`.
* Scope trigger: TIM2 is started from PA15 edges through Renode's timer
  trigger mode; the post-trigger TIM2 update was not verified against the
  stock's trigger search (the stock shows "AUTO" acquisitions and correct
  measurements).
* USB host side (USBTMC/MSC) would need a real DWC2 model.
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

## Files the emulator reads (never writes)

* `firmware/re/dumps/full-20261013/flash_08000000_1MB.bin`, `otp_1FFF7800.bin`
* `private/memrw-kit-test/full/uid_1FFF7A10.bin` (`--uid` to point elsewhere)
* `firmware/re/dumps/spi_dumps/dm40_msc_full_2026-10-13.bin` (copied to `emu/out/disk.bin`)
* `dm40_v_2_6.dec.bin` (stock body), `firmware/dm40fw.py` (header words)
* fw-rs: `git worktree add --detach emu/out/fwrs-worktree HEAD` + `./build.sh native`
  (left in place for reuse; `git worktree remove emu/out/fwrs-worktree` to drop it)
