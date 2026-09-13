#![allow(clippy::while_let_on_iterator)]

use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn compact_subcommand_json_fails_fast_when_stdin_closed() {
    let workspace = unique_temp_dir("compact-nontty-json");
    let config_home = workspace.join("config-home");
    let home = workspace.join("home");
    fs::create_dir_all(&workspace).expect("workspace should exist");
    fs::create_dir_all(&config_home).expect("config home should exist");
    fs::create_dir_all(&home).expect("home should exist");

    let output = run_claw_closed_stdin_with_timeout(
        &workspace,
        &config_home,
        &home,
        &["compact", "--output-format", "json"],
        Duration::from_secs(2),
    );

    assert!(
        !output.status.success(),
        "compact json should fail non-zero"
    );
    // #819/#820/#823: JSON abort envelopes route to stdout
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(
        stderr.trim().is_empty() || !stderr.trim_start().starts_with('{'),
        "compact json should not emit JSON envelope to stderr (#819/#820/#823): {stderr}"
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf8");
    let parsed: Value =
        serde_json::from_str(stdout.trim()).expect("stdout should be JSON error envelope");
    assert_eq!(parsed["status"], "error");
    assert_eq!(parsed["error_kind"], "interactive_only");
    assert_eq!(parsed["action"], "abort");
    assert!(
        parsed["message"]
            .as_str()
            .unwrap_or_default()
            .contains("disco compact"),
        "message should name compact: {parsed}"
    );
    // #749: hint must be non-empty (was null before fix — same class as #738/#745/#746)
    let hint = parsed["hint"].as_str().unwrap_or("");
    assert!(
        !hint.is_empty(),
        "compact interactive-only JSON must have non-empty hint (#749); got: {parsed}"
    );
    assert!(
        hint.contains("/compact") || hint.contains("--resume"),
        "hint should mention /compact or --resume: {hint}"
    );

    fs::remove_dir_all(&workspace).expect("workspace cleanup should succeed");
}

#[test]
fn compact_subcommand_text_fails_fast_when_stdin_closed() {
    let workspace = unique_temp_dir("compact-nontty-text");
    let config_home = workspace.join("config-home");
    let home = workspace.join("home");
    fs::create_dir_all(&workspace).expect("workspace should exist");
    fs::create_dir_all(&config_home).expect("config home should exist");
    fs::create_dir_all(&home).expect("home should exist");

    let output = run_claw_closed_stdin_with_timeout(
        &workspace,
        &config_home,
        &home,
        &["compact"],
        Duration::from_secs(2),
    );

    assert!(
        !output.status.success(),
        "compact text should fail non-zero"
    );
    assert!(
        output.stdout.is_empty(),
        "compact text should not start a prompt/spinner on stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr should be utf8");
    assert!(
        stderr.contains("[error-kind: interactive_only]"),
        "{stderr}"
    );
    assert!(stderr.contains("disco compact"), "{stderr}");

    fs::remove_dir_all(&workspace).expect("workspace cleanup should succeed");
}

fn run_claw_closed_stdin_with_timeout(
    cwd: &std::path::Path,
    config_home: &std::path::Path,
    home: &std::path::Path,
    args: &[&str],
    timeout: Duration,
) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_disco"))
        .current_dir(cwd)
        .env_clear()
        .env("CLAW_CONFIG_HOME", config_home)
        .env("HOME", home)
        .env("NO_COLOR", "1")
        .env("PATH", "/usr/bin:/bin")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .args(args)
        .spawn()
        .expect("disco should launch");

    let start = Instant::now();
    loop {
        if child.try_wait().expect("try_wait should succeed").is_some() {
            return child.wait_with_output().expect("output should collect");
        }
        if start.elapsed() > timeout {
            let _ = child.kill();
            let output = child
                .wait_with_output()
                .expect("killed output should collect");
            panic!(
                "disco did not exit within {:?}\nstdout:\n{}\nstderr:\n{}",
                timeout,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn unique_temp_dir(label: &str) -> PathBuf {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_millis();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "claw-compact-{label}-{}-{millis}-{counter}",
        std::process::id()
    ))
}
