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
- Text in fonts under 14 px (Chinese: under 15 px), and any translation wider
  than its box or with a glyph missing from its font, stays English.

## Sources and licences

- Glyphs: [SIL OFL 1.1](OFL.txt), Noto Sans CJK SC (Chinese), Liberation Sans
  (Cyrillic). Accented Latin letters are the stock letters plus an accent.
- Wording marked `R`: OpenDM40-Chinese, MIT, Copyright (c) 2026
  xingyan4117-sketch. Only its texts are used, none of its glyphs or code.
