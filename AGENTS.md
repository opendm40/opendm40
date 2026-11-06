# Notes for coding agents

Build, tests and CI: [firmware/README.md](firmware/README.md). Contribution
rules: [CONTRIBUTING.md](CONTRIBUTING.md).

## Developer image for tests

`./build.sh` builds the release image, without the developer `DBG:` commands
(`DBG:READout?`, `DBG:IDLE`, `DBG:FLASH?`, ...; Cargo feature `dbg-dev`).
Device checks and emulator tests need the developer image:
`tools/reload.sh` and `emu/run.py` build it; elsewhere `./build.sh --dev`.
For `reload.sh --image FILE` pass a `--dev` build
(`dm40_opendm40_v<version>-dev.bin`), and leave the meter on a developer
image after a release-image check.

## Docs in three languages

The README and the user manual exist in English, Russian and Chinese:

| English | Russian | Chinese |
|---|---|---|
| [README.md](README.md) | [README.ru.md](README.ru.md) | [README.zh.md](README.zh.md) |
| [docs/manual/README.md](docs/manual/README.md) | [docs/manual/README.ru.md](docs/manual/README.ru.md) | [docs/manual/README.zh.md](docs/manual/README.zh.md) |

- Every change to one of them is made in all languages in the same commit
  or pull request. Translate it; do not leave a TODO or English text in a
  translated file.
- Same structure in all: the same headings at the same levels, tables, list
  items, images, links and code. Intra-document links in a translated file
  point to its translated headings; links to English-only documents say
  «(на английском)» / （英文）.
- Keep the terms of the glossary at the end of the Russian and the Chinese
  manual (an HTML comment); on-screen labels, keys, SCPI commands and file
  names stay in English.
- Chinese: no line breaks inside a paragraph (GitHub shows them as spaces);
  a space around `**` where the bold text starts or ends with punctuation
  (`**LIVE?** 后`), otherwise the stars show literally.
- [`firmware/tools/doc-lang-check.sh`](firmware/tools/doc-lang-check.sh)
  enforces this in CI (Doc checks): run
  `firmware/tools/doc-lang-check.sh origin/main` before a push. For a typo
  fix in one language only, add the line `Doc-lang: one-language` to the
  commit message or the PR description.
