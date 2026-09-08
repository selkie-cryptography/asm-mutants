//! End-to-end runs of `cargo-asm-mutants` against the `testdata/carry`
//! fixture.

use std::{
    fs,
    path::Path,
    process::{Command, Output},
};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    for (path, contents) in [
        (
            "Cargo.toml",
            include_str!("../testdata/carry/Cargo.toml.in"),
        ),
        ("src/lib.rs", include_str!("../testdata/carry/src/lib.rs")),
        (
            "src/select.s",
            include_str!("../testdata/carry/src/select.s"),
        ),
    ] {
        fs::write(dir.path().join(path), contents).unwrap();
    }
    dir
}

fn run(args: &[&str]) -> Output {
    let fixture = fixture();
    run_in(fixture.path(), args)
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_cargo-asm-mutants"))
        .arg("-d")
        .arg(dir)
        // The fixture has no dependencies. Keep nested Cargo invocations
        // independent of registry availability.
        .env("CARGO_NET_OFFLINE", "true")
        .args(args)
        .output()
        .expect("run cargo-asm-mutants")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn help_version_and_usage_errors_have_distinct_exit_codes() {
    let empty = tempfile::tempdir().unwrap();
    for prefix in [vec![], vec!["asm-mutants"]] {
        for option in ["--help", "--version", "--unknown-option"] {
            let output = Command::new(env!("CARGO_BIN_EXE_cargo-asm-mutants"))
                .current_dir(empty.path())
                .args(&prefix)
                .arg(option)
                .output()
                .unwrap();
            match option {
                "--help" => {
                    assert!(output.status.success());
                    assert!(stdout(&output).contains("cargo asm-mutants"));
                }
                "--version" => {
                    assert!(output.status.success());
                    assert!(stdout(&output).contains(env!("CARGO_PKG_VERSION")));
                }
                _ => {
                    assert_eq!(output.status.code(), Some(1));
                    assert!(
                        String::from_utf8_lossy(&output.stderr).contains("unexpected argument")
                    );
                }
            }
        }
    }
}

/// Every mutant of the fixture, in source order: the included `.s` first,
/// since its `global_asm!` line precedes the `asm!` block.
const ALL: &str = concat!(
    "src/select.s:5:5: replace \"cmp x0, x1\" with \"nop\"\n",
    "src/select.s:6:5: replace \"csel x0, x0, x1, lo\" with \"nop\"\n",
    "src/select.s:6:5: replace \"csel x0, x0, x1, lo\" with \"mov x0, x0\"\n",
    "src/select.s:6:5: replace \"csel x0, x0, x1, lo\" with \"mov x0, x1\"\n",
    "src/select.s:7:5: replace \"ret\" with \"nop\"\n",
    "src/lib.rs:20:14: replace \"adds {sum}, {a}, {b}\" with \"nop\"\n",
    "src/lib.rs:20:14: replace \"adds {sum}, {a}, {b}\" with \"subs {sum}, {a}, {b}\"\n",
    "src/lib.rs:21:14: replace \"adc {carry}, xzr, xzr\" with \"nop\"\n",
    "src/lib.rs:21:14: replace \"adc {carry}, xzr, xzr\" with \"add {carry}, xzr, xzr\"\n",
    "src/lib.rs:21:14: replace \"adc {carry}, xzr, xzr\" with \"add {carry}, xzr, xzr; add {carry}, {carry}, #1\"\n",
);

#[test]
fn lists_files() {
    let output = run(&["--list-files"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output), "src/select.s\nsrc/lib.rs\n");
}

#[test]
fn lists_every_mutant_in_source_order() {
    let output = run(&["--list", "--no-shuffle"]);
    assert!(output.status.success());
    assert_eq!(stdout(&output), ALL);
}

#[test]
fn filters_by_operator_regex_and_glob() {
    let flags = stdout(&run(&["--list", "--operators", "flags"]));
    assert_eq!(flags.lines().count(), 4, "{flags}");
    assert!(
        flags
            .lines()
            .all(|l| l.contains("mov x0") || l.contains("add {carry}"))
    );

    let adc = stdout(&run(&["--list", "-F", "adc", "-E", "#1"]));
    assert_eq!(
        adc,
        concat!(
            "src/lib.rs:21:14: replace \"adc {carry}, xzr, xzr\" with \"nop\"\n",
            "src/lib.rs:21:14: replace \"adc {carry}, xzr, xzr\" with \"add {carry}, xzr, xzr\"\n",
        )
    );

    let gas_only = stdout(&run(&["--list", "-f", "*.s"]));
    assert_eq!(gas_only.lines().count(), 5, "{gas_only}");
    assert!(gas_only.lines().all(|l| l.starts_with("src/select.s")));
    let no_gas = stdout(&run(&["--list", "-e", "select.s"]));
    assert_eq!(no_gas.lines().count(), 5, "{no_gas}");
    assert!(no_gas.lines().all(|l| l.starts_with("src/lib.rs")));
    assert_eq!(
        stdout(&run(&["--list-files", "-f", "src/*.s"])),
        "src/select.s\n"
    );
}

#[test]
fn shards_partition_the_list() {
    let mut joined = String::new();
    for k in 0..3 {
        joined.push_str(&stdout(&run(&["--list", "--shard", &format!("{k}/3")])));
    }
    assert_eq!(joined, ALL);

    let mut round_robin = String::new();
    for k in 0..3 {
        round_robin.push_str(&stdout(&run(&[
            "--list",
            "--sharding",
            "round-robin",
            "--shard",
            &format!("{k}/3"),
        ])));
    }
    assert_eq!(round_robin.lines().count(), ALL.lines().count());
    assert_ne!(round_robin, ALL);
}

#[test]
fn in_diff_keeps_touched_lines_only() {
    let diff = concat!(
        "--- a/src/lib.rs\n",
        "+++ b/src/lib.rs\n",
        "@@ -21,1 +21,1 @@\n",
        "-            \"adc {carry}, xzr, xzr\",\n",
        "+            \"adc {carry}, xzr, xzr\",\n",
    );
    let path =
        std::env::temp_dir().join(format!("asm-mutants-in-diff-{}.diff", std::process::id()));
    fs::write(&path, diff).unwrap();
    let listed = stdout(&run(&["--list", "--in-diff", path.to_str().unwrap()]));
    fs::remove_file(path).unwrap();
    assert_eq!(listed.lines().count(), 3, "{listed}");
    assert!(
        listed.lines().all(|l| l.starts_with("src/lib.rs:21:")),
        "{listed}"
    );
}

#[test]
fn json_and_diff_listings() {
    let json = stdout(&run(&["--list", "--json", "-F", "mov x0"]));
    let records: serde_json::Value = serde_json::from_str(&json).unwrap();
    let records = records.as_array().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0]["file"], "src/select.s");
    assert_eq!(records[0]["line"], 6);
    assert_eq!(records[0]["column"], 5);
    assert_eq!(records[0]["genre"], "csel");
    assert_eq!(records[0]["replacement"], "mov x0, x0");
    assert!(
        records[0]["diff"]
            .as_str()
            .unwrap()
            .starts_with("--- src/select.s\n")
    );

    let with_diff = stdout(&run(&["--list", "--diff", "-F", "mov x0, x0"]));
    assert!(
        with_diff.contains("-    csel x0, x0, x1, lo\n+    mov x0, x0\n"),
        "{with_diff}"
    );
}

#[test]
fn baseline_failures_return_four_and_preserve_the_report() {
    for source in [
        "compile_error!(\"baseline build failure\");\n",
        "#[test]\nfn baseline_failure() { panic!(\"baseline test failure\"); }\n",
    ] {
        let fixture = fixture();
        let lib = fixture.path().join("src/lib.rs");
        let original = fs::read_to_string(&lib).unwrap();
        fs::write(&lib, format!("{original}\n{source}")).unwrap();
        let out = tempfile::tempdir().unwrap();
        let output = run_in(
            fixture.path(),
            &[
                "--operators",
                "carry",
                "--no-times",
                "-o",
                out.path().to_str().unwrap(),
            ],
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(
            output.status.code(),
            Some(4),
            "{}\n{stderr}",
            stdout(&output)
        );
        assert!(stderr.contains("baseline failed; see"), "{stderr}");
        let report = out.path().join("asm-mutants.out");
        let outcomes: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(report.join("outcomes.json")).unwrap())
                .unwrap();
        assert_eq!(outcomes["total_mutants"], 2);
        assert_eq!(outcomes["outcomes"].as_array().unwrap().len(), 1);
        assert_eq!(outcomes["outcomes"][0]["scenario"], "Baseline");
        assert!(!outcomes["end_time"].as_str().unwrap().is_empty());
        assert!(report.join("log/baseline.log").exists());
    }
}

/// Builds and tests the fixture, so only where its `asm!` compiles.
#[cfg(target_arch = "aarch64")]
#[test]
fn runs_the_carry_mutants() {
    let out = std::env::temp_dir().join(format!("asm-mutants-run-{}", std::process::id()));
    fs::create_dir_all(&out).unwrap();
    let output = run(&[
        "--operators",
        "carry",
        "-F",
        "adc",
        "--no-times",
        "--caught",
        "-o",
        out.to_str().unwrap(),
    ]);
    let printed = stdout(&output);
    assert_eq!(output.status.code(), Some(2), "{printed}");
    assert!(printed.contains("Found 2 mutants to test\n"), "{printed}");
    assert!(
        printed.contains("ok       Unmutated baseline\n"),
        "{printed}"
    );
    assert!(
        printed.contains(
            "MISSED   src/lib.rs:21:14: replace \"adc {carry}, xzr, xzr\" with \"add {carry}, xzr, xzr\"\n"
        ),
        "{printed}"
    );
    assert!(printed.contains("caught   src/lib.rs:21:14:"), "{printed}");
    assert!(
        printed.contains("2 mutants tested: 1 caught, 1 missed\n"),
        "{printed}"
    );

    let dir = out.join("asm-mutants.out");
    assert_eq!(
        fs::read_to_string(dir.join("missed.txt"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(
        fs::read_to_string(dir.join("caught.txt"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    let outcomes: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.join("outcomes.json")).unwrap()).unwrap();
    assert_eq!(outcomes["total_mutants"], 2);
    assert_eq!(outcomes["missed"], 1);
    assert_eq!(outcomes["caught"], 1);
    assert_eq!(outcomes["outcomes"][0]["scenario"], "Baseline");
    assert_eq!(outcomes["outcomes"][0]["summary"], "Success");
    assert!(dir.join("log/baseline.log").exists());
    assert!(dir.join("mutants.json").exists());
    assert!(dir.join("lock.json").exists());
    fs::remove_dir_all(out).unwrap();
}
