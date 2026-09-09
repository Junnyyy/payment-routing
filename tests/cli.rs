use std::process::Command;

#[test]
fn help_is_available_without_a_terminal() {
    for args in [vec![], vec!["--help"], vec!["-h"]] {
        let output = Command::new(env!("CARGO_BIN_EXE_payment-routing"))
            .args(args)
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("cargo run --locked -- --demo"));
        assert!(!stdout.contains('\u{1b}'));
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn bad_arguments_fail_without_entering_terminal_mode() {
    let output = Command::new(env!("CARGO_BIN_EXE_payment-routing"))
        .arg("--unknown")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("expected --demo or --help"));
}

#[test]
fn demo_rejects_piped_io_without_escape_sequences_or_panic() {
    let output = Command::new(env!("CARGO_BIN_EXE_payment-routing"))
        .arg("--demo")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("requires an interactive terminal"));
    assert!(!stderr.contains("panicked"));
    assert!(!stderr.contains('\u{1b}'));
}

#[test]
fn evaluation_is_headless_reproducible_and_reports_censoring() {
    let args = [
        "--evaluate",
        "--scenario",
        "missed-connection",
        "--seed",
        "42",
        "--ticks",
        "3",
        "--drain",
        "8",
        "--format",
        "csv",
    ];
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_payment-routing"))
            .args(args)
            .output()
            .unwrap()
    };
    let first = run();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(first.stdout, run().stdout);
    assert!(first.stderr.is_empty());
    let text = String::from_utf8(first.stdout).unwrap();
    assert!(text.contains("\"aggregate\""));
    assert!(!text.contains('\u{1b}'));
    let censored = Command::new(env!("CARGO_BIN_EXE_payment-routing"))
        .args([
            "--evaluate",
            "--scenario",
            "reservation-trap",
            "--seed",
            "42",
            "--ticks",
            "1",
            "--drain",
            "0",
        ])
        .output()
        .unwrap();
    assert!(!censored.status.success());
    assert!(String::from_utf8_lossy(&censored.stdout).contains("censored"));
    assert!(String::from_utf8_lossy(&censored.stderr).contains("censored or errored"));
}

#[test]
fn evaluation_rejects_invalid_options_before_output() {
    for args in [
        vec!["--ticks", "0"],
        vec!["--seeds", "1,1"],
        vec!["--seed", "1", "--seeds", "2"],
        vec!["--strategies", "static,static"],
        vec!["--scenario", "missing"],
        vec!["--format", "json"],
        vec!["--drain", "-1"],
        vec!["--ticks"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_payment-routing"))
            .arg("--evaluate")
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.contains(&0x1b));
    }
}
