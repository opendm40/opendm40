# Datasheets and application notes

**Copyright.** Every PDF in this folder is © its manufacturer and is
redistributed here for reference only. These files are **not** covered by
this repository's MIT license. The manufacturers' redistribution terms have
not been checked.

What the parts do in the DM40: [../README.md](../README.md).

## Hycon Technology (HY3131 DMM front end)

Manufacturer: HYCON Technology Corp. (www.hycontek.com). The four files were
downloaded by the repo owner in October 2026; the source URL was not
recorded. Hycon's download path for the original names is
`https://www.hycontek.com/wp-content/uploads/<name>.pdf`. Public mirrors of
some of them are listed in [emu/hy3131/RESEARCH.md](../../emu/hy3131/RESEARCH.md) 1.

| File | Document (as printed) | Original name | md5 |
|---|---|---|---|
| [hycon-hy3131-datasheet-v10.pdf](hycon-hy3131-datasheet-v10.pdf) | HY3130/HY3131 Datasheet, "5,000/50,000 Counts DMM Analog Front End", DS-HY3131-V10_EN, 42 pages, PDF dated 2014-11-12 | `DS-HY3131_EN.pdf` | 4f53dcb761aa637790b4b12d7e1a40d0 |
| [hycon-hy313x-spi-appnote-apn-hy3131001.pdf](hycon-hy313x-spi-appnote-apn-hy3131001.pdf) | "HY313x Series SPI Communication" application note, HYCON FAE Dept., 2017-11-17, 7 pages | `APN-HY3131001_EN.pdf` | 3313c37544344a988c823c2ba4854096 |
| [hycon-hy313x-eva-test-tool-manual-apd-dmm001.pdf](hycon-hy313x-eva-test-tool-manual-apd-dmm001.pdf) | "HY313X EVA Test Tool User Manual", APD-DMM001-V02_EN, 2012, 24 pages (ENOB / noise-free resolution test tool) | `APD-DMM001_EN.pdf` | 0fddee10b6c92442e1ec0e17e8a57de1 |
| [hycon-hy313x-configurations-apd-dmm003.pdf](hycon-hy313x-configurations-apd-dmm003.pdf) | "HY313X Configurations", APD-DMM003-V05_EN, 2020, 55 pages (block diagrams per function) | `APD-DMM003_EN.pdf` | 2aed1e738acc1c47526704636b7b1f23 |

[`emu/hy3131/RESEARCH.md`](../../emu/hy3131/RESEARCH.md) cites APD-DMM003 **V02** (2012, the essentialscrap
mirror); the copy here is the newer **V05**.

## Other parts

Retrieved 2026-10-24 from the URL given (the manufacturer's site unless
noted). "On the board" says how we know the part is in the DM40 (details in
[../README.md](../README.md) "Main parts").

Rows marked *link only* are too large to keep in the repo: download them from
the source URL.

| File | Part, document (as printed) | Manufacturer | Source URL | On the board |
|---|---|---|---|---|
| [gigadevice-gd32f427xx-datasheet-rev1.9.pdf](gigadevice-gd32f427xx-datasheet-rev1.9.pdf) | GD32F427xx Arm Cortex-M4 32-bit MCU Datasheet, Rev 1.9 (Jan. 2026) | GigaDevice Semiconductor Inc. | https://download.gigadevice.com/Datasheet/GD32F427xx_Datasheet_Rev1.9.pdf | photo marking GD32F427VGT6 |
| *link only* (28.6 MB, not stored) | GD32F4xx Arm Cortex-M4 32-bit MCU User Manual, Rev 3.3 (Aug. 2025), 1093 pages | GigaDevice Semiconductor Inc. | https://download.gigadevice.com/User_Manual/GD32F4xx_User_Manual_Rev3.3.pdf | (MCU family manual) |
| [`winbond-w25q128jv-revI.pdf`](winbond-w25q128jv-revI.pdf) | W25Q128JV 3V 128M-bit Serial Flash Memory with Dual/Quad SPI, Revision I (2021-08-23) | Winbond Electronics | https://www.winbond.com/resource-files/W25Q128JV%20RevI%2008232021%20Plus.pdf | code ([`spiflash.rs`](../../fw-rs/src/spiflash.rs)), BOM |
| *link only* (10.8 MB, not stored) | ILI9488 a-Si TFT LCD Single Chip Driver 320(RGB)x480, Specification V100 (ILI9488_IDT_V100_20121128) | ILI Technology Corp. (Ilitek) | https://www.crystalfontz.com/controllers/uploaded/ILI9488%20Data%20Sheet_100.pdf (Ilitek does not host it; Crystalfontz, a display vendor, redistributes Ilitek's own document "as is") | device ID 0x9488 read by fw-rs |
| [toppower-tp4056-cn.pdf](toppower-tp4056-cn.pdf) | TP4056 1A linear Li-ion battery charger datasheet (Chinese; no revision printed, PDF created 2017-08-16) | NanJing Top Power ASIC Corp. | http://www.tp-asic.com/res/tp-asic/pdres/202206/TP4056_42.pdf | photo marking TP4056 |
| [chipanalog-ca-is374x-v1.08.pdf](chipanalog-ca-is374x-v1.08.pdf) | CA-IS374x High-Speed Four-Channel Digital Isolators (CA-IS3740/3741/3742), Version 1.08 | Shanghai Chipanalog Microelectronics | https://e.chipanalog.com/Public/Uploads/uploadfile/files/20250123/CAIS374xdatasheetVersion1.08en20241217.pdf | photo marking CA-IS3741HW (3 forward + 1 reverse channel) |
| [vpsc-vps8504b-cn.pdf](vpsc-vps8504b-cn.pdf) | VPS8504B micro-power isolated supply driver, 2.8-6 VIN (Chinese; "Copyright May 2021") | Suzhou Yuante Semiconductor (brand VPSC, www.vpsct.cn) | https://datasheet.lcsc.com/datasheet/pdf/a3685b587675a0a8ad41e6bde475d210.pdf?productCode=C2846907 (LCSC, distributor: the maker's site offers no datasheet download) | photo marking VPS8504B |
| [hongfa-hfd4.pdf](hongfa-hfd4.pdf) | HFD4 Subminiature Signal Relay, 2026 Rev. 1.00 (single side stable and latching versions; the PDF's text layer is scrambled, read it as an image) | Xiamen Hongfa Electroacoustic | https://source.hongfa.com/Uploads/Product/PDF/HFD4_en.pdf | photo marking HFD4/3-LS |
| [3peak-3pa1030-reva1.pdf](3peak-3pa1030-reva1.pdf) | 3PA1030 Complete 10-Bit, 50-MSPS, CMOS ADC, Rev.A.1 (2024-12-30) | 3PEAK | https://static.3peak.com/res/doc/ds/Datasheet_3PA1030.pdf | BOM only (under the scope shield); 10-bit bus in code |
| [gainsil-gs8091.pdf](gainsil-gs8091.pdf) | GS8091/8092/8094/8091N/8092N 350 MHz CMOS Rail-to-Rail Output Opamps, March 2020 REV_V1 | Gainsil | http://www.gainsil.com/images/product/28_2.pdf | BOM only ("8091"), unconfirmed |
| [ti-cd74hc4051-schs122o.pdf](ti-cd74hc4051-schs122o.pdf) | CDx4HC405x, CD4HCT405x analog multiplexer, SCHS122O (rev. January 2026) | Texas Instruments | https://www.ti.com/lit/ds/symlink/cd74hc4051.pdf | BOM "HC4051" / RE header "HCF4051": the maker of the actual chip is unknown; this is a generic 4051 reference |

Not included:

- MAX6190 (1.25 V reference, BOM only): Analog Devices hosts it at
  https://www.analog.com/media/en/technical-documentation/data-sheets/MAX6190-MAX6198.pdf
  (not stored; download it by hand).
- TC1746B6 divider network: maker not identified.
- Hynitron CST touch controller (exact model unknown), Goodix GT1158, Jieli
  BP2A838: no official public datasheets found.
- SGM3157, RUH30J51M, EL3H7, FMMT619, LT61-1xx transformers, the power-path
  parts: not searched yet (markings from the BOM / chat reports only).
