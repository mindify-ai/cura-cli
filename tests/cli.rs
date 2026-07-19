use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_exposes_core_lifecycle() {
    Command::cargo_bin("cura")
        .unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "interactive CUDA environment manager",
        ))
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("doctor"));
}

#[test]
fn empty_home_lists_cleanly() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cura")
        .unwrap()
        .env("CURA_HOME", home.path())
        .args(["--no-interactive", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No CUDA environments installed"));
}

#[test]
fn doctor_has_stable_json_envelope() {
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cura")
        .unwrap()
        .env("CURA_HOME", home.path())
        .args(["--json", "doctor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"ok\""))
        .stdout(predicate::str::contains("\"platform\""));
}

#[test]
fn install_rejects_unsupported_host_before_network() {
    if cfg!(target_os = "linux") {
        return;
    }
    let home = tempfile::tempdir().unwrap();
    Command::cargo_bin("cura")
        .unwrap()
        .env("CURA_HOME", home.path())
        .args([
            "--no-interactive",
            "install",
            "cuda-12",
            "--profile",
            "runtime",
            "--yes",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("supported on Linux and WSL"));
}
