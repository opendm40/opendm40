# HY3131 comparison generator

Generates the per-mode tables and the raw-row appendix of
[`hy3131-firmware-comparison.md`](../../../hy3131-firmware-comparison.md).

| file | content |
|---|---|
| [`rows.json`](rows.json) | every source row: source, mode, label, registers R20..R36 |
| [`hydec.py`](hydec.py) | row → named fields decoder |
| [`apd.json`](apd.json) | Hycon APD-DMM003 switch readings |
| [`template.md`](template.md) | the doc with `{{TABLE:<mode>}}` and `{{APPENDIX}}` placeholders |
| [`gen.py`](gen.py) | fills the template |
| [`build_121gw.py`](build_121gw.py) | replaces the 121GW v1.02 rows in a rows file with rows read from the v2.05 / v1.02 images |

Edit prose in `template.md`, rows in `rows.json`, then:

```sh
cd firmware/docs/data/hy3131-comparison/tools
python3 gen.py template.md ../../../hy3131-firmware-comparison.md
```
