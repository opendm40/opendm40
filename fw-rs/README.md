# DM40 replacement firmware (Rust, RAM-loaded PoC)

Replacement firmware for the Alientek DM40 (GD32F427, Cortex-M4F @ 200 MHz).
Scope of this PoC: multimeter only, GUI matching the stock firmware but with the
main colour changed from orange/yellow to red. No settings screens, no
Bluetooth. Calibration and configuration are read from their existing locations;
nothing is written. USB host-side access uses USBTMC (added in a later step).

The image runs from SRAM, taking over from the stock firmware and
re-initialising the peripherals it needs. How the image is placed in RAM and
started is the loader's job (provided separately); this crate only produces the
image and defines the contract below.

## Load contract

- **Format:** bare binary, position-dependent, loaded at `0x20008000`.
- **Layout:** Cortex-M vector table at the start — word[0] = initial MSP
  (`0x20018000`), word[1] = reset entry (thumb bit set). A loader copies the
  image to `0x20008000` and branches to word[1].
- **Region:** `0x20008000`..`0x20018000` (64 KiB) inside the ~100 KiB of SRAM0
  the stock firmware leaves free in multimeter mode (free from `0x20005024`).
  The low 32 KiB of SRAM0 (stock scope capture buffer) is left untouched until
  the peripherals are re-initialised.
- **On entry:** reset sets MSP from word[0], masks interrupts (`cpsid i`), points
  `VTOR` at the in-SRAM table, zeroes `.bss` and calls `main`. The loader does
  not need to keep its interrupt handlers alive after the branch.

## Milestones

1. **(done)** Prove load + run + display: fill the screen red, keep the free
   watchdog (FWDGT, reload 0xAAAA at 0x40003000) fed now that the stock TIM7
   1 kHz ISR no longer runs. Reuses the FSMC/LCD setup the stock firmware
   already performed.
2. Re-init clocks, FSMC and the ILI9488 from scratch; text/number rendering.
3. HY3131 driver (bit-banged SPI) + measurement loop, calibration read from the
   stock settings record at 0x080C0000.
4. Multimeter GUI matching `firmware/re/analysis/` notes, main value in red.
5. USBTMC + SCPI over USBFS.

## Build

```
./build.sh      # -> dm40-fw.bin
```

Needs the pinned toolchain in `../toolchain` (rustup + stable +
`thumbv7em-none-eabihf`). `build.sh` sources `../toolchain/env.sh`.
