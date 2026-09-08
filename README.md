# cargo-asm-mutants

Mutation testing for inline assembly: the `asm!` gap in
[cargo-mutants](https://mutants.rs/).

cargo-mutants rewrites Rust syntax and never looks inside an `asm!`
template string, so a hand-scheduled kernel gets no mutants at all. This
tool mutates the template lines themselves, one instruction at a time,
rebuilds, and runs the tests per mutant. `global_asm!(include_str!("x.s"))`
files are mutated the same way.

The command line, configuration keys, report layout, and exit codes follow
cargo-mutants conventions. Assembly mutant records have their own fields;
see `--list --json` and `asm-mutants.out/outcomes.json` when adapting a report
consumer.

This is an early release with AArch64 scalar and NEON operators. See
[limitations](#limitations) before interpreting results.

## Install

Requires Rust 1.89 or newer. Install a released version from crates.io:

```sh
cargo install --locked cargo-asm-mutants
```

Or install from a checkout:

```sh
cargo install --locked --path .
```

Running mutants also requires Cargo and the compiler/linker needed by the
crate under test. Install [cargo-nextest](https://nexte.st/docs/installation/pre-built-binaries/)
separately to use `--test-tool nextest`.

## Use

```sh
cargo asm-mutants --list                     # every mutant, no builds
cargo asm-mutants                            # test them all
cargo asm-mutants -f src/arch/neon.rs          # one file
cargo asm-mutants --operators flags          # carry and select operators only
cargo asm-mutants --in-diff pr.diff           # only code a diff touches
cargo asm-mutants --shard 0/4 -j 2            # a quarter of the mutants, two at a time
cargo asm-mutants --test-tool nextest --cargo-test-arg=-E --cargo-test-arg='test(/neon/)'
```

Run from the crate or workspace root, or select it with `-d path/to/crate`.
Use `--help` for every option. `-C` adds an argument to both build and test
commands; `--cargo-test-arg` adds one to the test command. Arguments after
`--` are passed after `--` to the test runner, so nextest options such as
`-E` belong in `--cargo-test-arg`.

Each mutant is applied to a scratch copy of the tree, built with
`cargo test --no-run` (or `cargo nextest run --no-run`), then tested. A
build failure is `unviable`, a passing suite is `MISSED`. Missed mutants are
printed as they happen; `--caught` and `--unviable` print the rest.

Start with `--list --diff` to inspect the selected instructions. A missed
mutant can reveal a missing assertion, but it can also be equivalent to the
original code or sit in code that was never compiled or executed.

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

Command-line scalar values and operator selections override configuration.
File globs, regular expressions, and additional Cargo arguments combine
with the configured values. Use `--no-config` to ignore configuration, or
`--config path/to/asm-mutants.toml` to select a file.

## Output

`asm-mutants.out/` next to the crate (or under `--output`), a previous run
moved to `asm-mutants.out.old/`:

- `caught.txt`, `missed.txt`, `timeout.txt`, `unviable.txt`: one mutant name
  per line.
- `mutants.json`: every mutant tested, with its diff.
- `outcomes.json`: every scenario with its phases, in cargo-mutants' shape.
- `log/<file>_line_<n>_col_<c>.log`, `diff/...`: per mutant.
- `lock.json`.

| Exit code | Meaning |
|-----------|---------|
| 0 | No missed or timed-out mutants, or a successful listing/check |
| 1 | Usage or internal error |
| 2 | Missed mutants |
| 3 | Mutant build or test timeouts, including runs with missed mutants |
| 4 | The unmutated tree fails to build or test, or times out |

Exit code 0 can include unviable mutants or an empty selection. Review the
counts in `outcomes.json`; it does not establish complete test coverage.
Use a separate `--output` directory for each concurrent invocation.

## Timeouts

Per phase, from the baseline: tests get `timeout_multiplier` (1.5) times
the baseline test time with a `minimum_test_timeout` (20s) floor, builds get
`build_timeout_multiplier` (2.0) times the baseline build time, with the same
minimum floor. `--timeout` and `--build-timeout` set them outright.
With `--baseline skip` and no
explicit timeout, both fall back to 300s. The baseline itself gets up to an
hour per phase unless explicit timeouts are set. On Unix, a timed-out process
group is killed as a whole, so a spinning test binary dies with its Cargo
parent.

## Limitations

- AArch64 is the only supported instruction set. CI runs the mutation fixture
  on Linux ARM64 and Apple Silicon, and parsing/listing tests on Linux x86_64.
  Windows is not tested; its timeout handling only kills the Cargo process.
- Discovery reads source text without expanding macros or evaluating `cfg`.
  The mutated code must be in the compile graph and executed by the tests.
  Use `--features` and file filters to select the intended implementation;
  AArch64 code compiled out on another architecture can appear as missed.
- Templates must use ordinary quoted strings, one instruction per source
  line, after the `asm!(` or `global_asm!(` opening line. Raw strings,
  multiple instructions in one string, and macro-generated templates are
  not supported. An included assembly file must use a literal
  `global_asm!(include_str!("path.s"))` on one line. Generated `OUT_DIR`
  assembly is not discovered.
- Scratch builds copy the selected root. For a workspace, select its root
  and use `-C=-p -C=package-name` if needed. Path dependencies outside that
  root must remain accessible, and build inputs excluded by `.gitignore`
  may require `--gitignore false`.
- `--in-place` modifies the selected tree and forces one worker. Normal
  completion restores each file; interruption can leave a mutation applied.
  Keep a clean commit before using it.

## Development

See [CONTRIBUTING.md](https://github.com/selkie-cryptography/asm-mutants/blob/main/CONTRIBUTING.md)
for local checks and fixtures, and
[RELEASING.md](https://github.com/selkie-cryptography/asm-mutants/blob/main/RELEASING.md)
for release commands.

## License

Licensed under either [Apache-2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at
your option.
