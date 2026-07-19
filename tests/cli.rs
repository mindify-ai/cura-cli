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
            "interactive GPU environment manager",
        ))
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("metal"))
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
        .stdout(
            predicate::str::contains("No CUDA environments installed")
                .or(predicate::str::contains("Metal is managed by macOS")),
        );
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

#[test]
fn metal_status_has_stable_json_output() {
    Command::cargo_bin("cura")
        .unwrap()
        .args(["--json", "metal", "status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"status\": \"ok\""))
        .stdout(predicate::str::contains("\"supported\""))
        .stdout(predicate::str::contains("\"ready\""));
}

#[test]
fn metal_alias_rejects_cuda_only_options() {
    Command::cargo_bin("cura")
        .unwrap()
        .args([
            "--no-interactive",
            "install",
            "metal",
            "--profile",
            "runtime",
            "--yes",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("apply only to CUDA installations"));
}
