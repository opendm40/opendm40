# Contributing to opendm40

## Report a bug

Open an [issue](https://github.com/wirenboard/opendm40/issues/new/choose)
with the bug report form. Say:

- the model (DM40A, DM40B or DM40C) and the opendm40 version: both are on
  **Settings > More > More > Device**, and in the reply to `*IDN?`;
- what you did, what you expected and what the meter did;
- a screenshot if the screen shows it (a three-finger swipe saves one to
  `SHOTS/` on the USB disk, [Screenshots](docs/manual/README.md#screenshots));
- for remote control: the commands you sent and the reply to `SYST:ERR?`;
- if the meter hung or restarted: the output of
  `python3 firmware/tools/dm40dbg.py boot` (`DBG:BOOT?`).

Check the manual's [Known limitations](docs/manual/README.md#known-limitations)
first: what has not run on a real meter yet is listed there.

## Change the code

- Build, toolchain, the USB debug link, the tests and CI:
  [firmware/README.md](firmware/README.md). Every document:
  [docs/README.md](docs/README.md).
- Pull requests go against `main`. CI must pass: the build with its stack
  and memory checks, the host test suites, the doc checks and the version
  check.
- A change to the firmware needs a new version in
  [`firmware/Cargo.toml`](firmware/Cargo.toml)
  ([Releasing](firmware/README.md#releasing)) and, if users notice it, a
  line under `## Unreleased` in [CHANGELOG.md](CHANGELOG.md).
- Run the [emulator](emu/README.md) tests for changes to the multimeter, the
  boot or the USB disk; they need a flash dump of your own meter and are not
  in CI.
- Say in the pull request what you checked on a real meter, and on which
  model.
- Commit messages: `Area: what changed`, for example
  `Scope: keep the trigger level when the V/div changes`; the body says why.

## Documentation

The [user manual](docs/manual/README.md) is the model for the other
documents:

- one reader per document, in the order that reader needs;
- short sentences in the present tense; the names on the screen in **bold**;
- links to files and sections, not bare file names;
- mark only what is *inferred* or *unknown*; everything else is stated as
  fact, without "verified";
- no dates, commit hashes or session notes in reference text; untested
  features go to the manual's Known limitations.

A change to the SCPI commands updates
[docs/manual/scpi-reference.md](docs/manual/scpi-reference.md)
(`firmware/tools/scpi-doc-check` compares it with the parser).
`python3 firmware/tools/linkcheck.py` checks every relative link and anchor.

## Licence

Contributions are under the [MIT licence](LICENSE) of the project. Do not add
material you may not publish under it, such as vendor code, datasheets or
data read from your own meter (calibration, chip ID, flash dumps).
