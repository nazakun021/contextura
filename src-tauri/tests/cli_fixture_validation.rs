use std::fs;
use std::process::Command;

#[test]
fn debug_cli_validates_corpus_fixtures_without_a_local_model() {
    let fixture_dir = std::env::temp_dir().join(format!(
        "contextura-cli-fixture-validation-{}",
        std::process::id()
    ));
    fs::create_dir_all(&fixture_dir).expect("fixture directory should be created");
    fs::write(fixture_dir.join("case.png"), []).expect("PNG fixture should be created");
    fs::write(fixture_dir.join("case.expected.json"), "{}")
        .expect("expectation fixture should be created");

    let output = Command::new(env!("CARGO_BIN_EXE_contextura"))
        .args([
            "--debug-cli",
            "--validate-corpus-fixtures",
            fixture_dir.to_str().expect("fixture path should be UTF-8"),
        ])
        .output()
        .expect("Contextura CLI should run");

    let _ = fs::remove_dir_all(&fixture_dir);

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("Validated 1 corpus fixture pair"));
}

#[test]
fn cli_uses_contextura_data_dir_for_headless_model_state() {
    let data_dir =
        std::env::temp_dir().join(format!("contextura-cli-data-dir-{}", std::process::id()));
    let _ = fs::remove_dir_all(&data_dir);

    let output = Command::new(env!("CARGO_BIN_EXE_contextura"))
        .arg("--list-models")
        .env("CONTEXTURA_DATA_DIR", &data_dir)
        .output()
        .expect("Contextura CLI should run");

    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(data_dir.join("settings.json").is_file());

    let _ = fs::remove_dir_all(&data_dir);
}
