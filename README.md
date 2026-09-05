# cargo-asm-mutants

Mutation testing for inline assembly: the `asm!` gap in
[cargo-mutants](https://mutants.rs/).

cargo-mutants rewrites Rust syntax and never looks inside an `asm!`
template string, so a hand-scheduled kernel gets no mutants at all. This
tool mutates the template lines themselves, one instruction at a time,
rebuilds, and runs the tests per mutant. `global_asm!(include_str!("x.s"))`
files are mutated the same way.

The command line, configuration file, output directory, and exit codes
follow cargo-mutants, so anything that consumes `mutants.out/` can consume
`asm-mutants.out/`.

## Install

```sh
cargo install --git https://github.com/selkie-cryptography/asm-mutants
```

## Use

```sh
cargo asm-mutants --list                     # every mutant, no builds
cargo asm-mutants                            # test them all
cargo asm-mutants -f src/arch/neon.rs        # one file
cargo asm-mutants --operators flags          # carry and select operators only
cargo asm-mutants --in-diff pr.diff          # only code a diff touches
cargo asm-mutants --shard 0/4 -j 2           # a quarter of the mutants, two at a time
cargo asm-mutants --test-tool nextest -- -E 'test(/neon/)'
```

Each mutant is applied to a scratch copy of the tree, built with
`cargo test --no-run` (or `cargo nextest run --no-run`), then tested. A
build failure is `unviable`, a passing suite is `MISSED`. Missed mutants are
printed as they happen; `--caught` and `--unviable` print the rest.

The mutated code has to be in the compile graph on the machine running the
tests. A kernel behind `#[cfg(target_arch = "aarch64")]` is only tested on
aarch64; elsewhere every mutant of it builds and passes, and is reported as
missed.

## Operators

Two families, both on by default. `--operators` (or `operators` in the
config file) takes family names, operator names, or `carry` for both carry
directions.

`coverage` changes what an instruction computes. What a vector kernel needs.

| operator    | mutation                                                                 |
|-------------|--------------------------------------------------------------------------|
| `delete`    | the instruction becomes `nop`                                            |
| `mnemonic`  | `add`/`sub`, `mla`/`mls`, `sqrdmulh`/`sqdmulh`, `cbnz`/`cbz`, `zip1`/`zip2`, ... |
| `swap`      | the last two operands of a non-commutative op, or the registers of an `ldp`/`stp` |
| `immediate` | every decimal immediate by +-1, +-16 on load/store offsets               |

`flags` changes what a flag-dependent instruction sees, after
[Go Assembly Mutation Testing](https://words.filippo.io/assembly-mutation/).
What a carry-chain field library needs.

| operator      | mutation                                                              |
|---------------|-----------------------------------------------------------------------|
| `carry-clear` | `adcs` to `adds`; `adc` to `add`; `sbcs` and `sbc` see the carry clear |
| `carry-set`   | `subs zr, zr, zr` before `adcs`; `add d, d, #1` after `adc`; mirrored for `sbcs`/`sbc` |
| `csel`        | `mov` from either input                                               |

In a Rust template, a replacement that drops a `{operand}` keeps it in a
trailing asm comment, since rustc rejects an operand the template never
names.

AArch64 is the only instruction set so far.

## Configuration

`.cargo/asm-mutants.toml`, with cargo-mutants' keys where they apply:

```toml
test_tool = "nextest"
profile = "mutants"
additional_cargo_args = ["--features", "expose-internals"]
copy_target = true
timeout_multiplier = 2.0
minimum_test_timeout = 90
operators = ["coverage", "flags"]
# Equivalent mutants, matched against `--list` names.
exclude_re = ['stp q\d+, q\d+, \[\{ptr\}\], #(16|48)"$']
```

Also `additional_cargo_test_args`, `examine_globs`, `exclude_globs`,
`examine_re`, `build_timeout`, `build_timeout_multiplier`, `timeout`,
`copy_vcs`, `gitignore`, `output`.

## Output

`asm-mutants.out/` next to the crate (or under `--output`), a previous run
moved to `asm-mutants.out.old/`:

- `caught.txt`, `missed.txt`, `timeout.txt`, `unviable.txt`: one mutant name
  per line.
- `mutants.json`: every mutant tested, with its diff.
- `outcomes.json`: every scenario with its phases, in cargo-mutants' shape.
- `log/<file>_line_<n>_col_<c>.log`, `diff/...`: per mutant.
- `lock.json`.

Exit codes: 0 all caught, 1 usage or internal error, 2 missed mutants, 3
timeouts, 4 the unmutated tree fails to build or test.

## Timeouts

Per phase, from the baseline: tests get `timeout_multiplier` (1.5) times
the baseline test time with a `minimum_test_timeout` (20s) floor, builds get
`build_timeout_multiplier` (2.0) times the baseline build time. `--timeout`
and `--build-timeout` set them outright. With `--baseline skip` and no
explicit timeout, both fall back to 300s. A timed-out process group is
killed as a whole, so a spinning test binary dies with its cargo.

## License

Apache-2.0 OR MIT.
