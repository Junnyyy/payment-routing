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
