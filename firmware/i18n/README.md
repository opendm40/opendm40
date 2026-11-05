# UI languages

A localized image holds one language next to English; **Settings > Display >
Language** switches. The default image is English only.

- `<lang>.tsv`: English, translation, source. `S`: the stock firmware's wording;
  `R`: the wording of [OpenDM40-Chinese](https://github.com/xingyan4117-sketch/OpenDM40-Chinese);
  `M`: machine-written, *needs a native speaker's review*; `E`: English on
  purpose in this language. A new UI string goes to all four tables or to
  [english.txt](english.txt) ([`tools/lang-check`](../tools/lang-check/run.sh)).
- [`tools/mklang.py`](../tools/mklang.py) `<lang>` writes `src/lang_<lang>.rs`.
  The fonts are not in the repo (Debian: fonts-noto-cjk, fonts-liberation).
- [font-use.tsv](font-use.tsv): which font draws which string, from
  [`tools/lang-render`](../tools/lang-render/run.sh) (`lang.log`, kind `drawn`).
- Text in fonts under 12 px (Chinese: under 15 px), and any translation wider
  than its box or with a glyph missing from its font, stays English.

## Sources and licences

- Glyphs: [SIL OFL 1.1](OFL.txt), Noto Sans CJK SC (Chinese), Liberation Sans
  (Cyrillic). Accented Latin letters are the stock letters plus an accent.
- Wording marked `R`: OpenDM40-Chinese, MIT, Copyright (c) 2026
  xingyan4117-sketch. Only its texts are used, none of its glyphs or code.

## Chinese terms

Where sources disagree, the established instrument-brand term wins; the stock wording stays first where it is itself the standard term.

| English | Chinese | Source |
|---|---|---|
| Burden voltage | 负荷电压 | Fluke China (287/289, 15B MAX / 17B MAX); community firmware 负载压降 reads as a drop across the load |
| Runt | 欠幅 | Rigol MSO5000 欠幅脉冲 |
| Trigger source | 触发源 | Rigol MSO5000; 信号源 means a signal generator |
| Pulse trigger | 脉宽 | Rigol MSO5000 脉宽触发 |
| USB disk | U盘模式 | *inferred*: this row picks read-only or editable; stock USB连接 is another function |
| Offset | 偏移 | Rigol DG4000, MSO5000 |

The zh manual follows these terms: 负担电压 -> 负荷电压, 偏置 -> 偏移, 矮脉冲 -> 欠幅脉冲.
