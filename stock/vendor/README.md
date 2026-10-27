# Vendor firmware

**Licence notice.** [`dm40_v_2_6.bin`](dm40_v_2_6.bin) is Alientek's original firmware update
file for the DM40 multimeter (V2.6, the latest release), as downloaded from
the vendor's website. It is redistributed **unmodified**, for reference, to
return a meter to the stock firmware, and to make the builds and the emulator
in this repository reproducible. It is **not** covered by the MIT licence of
this repository, no licence to it is granted here, and all rights in it
belong to its owner. This repository is not affiliated with or endorsed by
Alientek.

| Version | File | Size (bytes) | sha256 |
|---|---|---:|---|
| V2.6 | [`dm40_v_2_6.bin`](dm40_v_2_6.bin) | 495140 | `5c2efe345369238604b8001af99ab089f82ff405fa91d5b8368c0086484038ac` |

Retrieved on 2026-10-13. Older versions (V1.6 .. V2.5) are kept in
a separate tools repository (`vendor/`).

- **Back to stock:** copy this file onto the FS-loader's USB disk as the only
  `dm40*.bin` and press "update" (see [the manual, Install a file from the update loader](../../docs/manual/README.md#install-a-file-from-the-update-loader)).
- **Decoded body** (needed by `emu/run.py stock`, [`firmware/tools/mkstockfont.py`](../../firmware/tools/mkstockfont.py)
  and the patch kit), written to the repo root, which git ignores:

  ```
  python3 stock/dm40fw.py decode stock/vendor/dm40_v_2_6.bin dm40_v_2_6.dec.bin
  ```
