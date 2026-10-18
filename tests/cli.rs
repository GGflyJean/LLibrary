use std::process::Command;

fn pdf_bytes() -> Vec<u8> {
    use lopdf::{dictionary, Document, Object};
    let mut document = Document::with_version("1.5");
    let pages_id = document.new_object_id();
    let page_id = document.add_object(dictionary! {
        "Type" => "Page",
        "Parent" => pages_id,
        "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
    });
    document.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! {
            "Type" => "Pages",
            "Kids" => vec![page_id.into()],
            "Count" => 1,
        }),
    );
    let catalog_id = document.add_object(dictionary! {
        "Type" => "Catalog",
        "Pages" => pages_id,
    });
    document.trailer.set("Root", catalog_id);
    let mut output = Vec::new();
    document.save_to(&mut output).unwrap();
    output
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_llibrary")
}

#[test]
fn help_lists_commands() {
    let output = Command::new(binary()).arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("scan"));
    assert!(stdout.contains("sources"));
}

#[test]
fn cli_adds_source_and_scans_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    std::fs::create_dir_all(root.join("中华书局")).unwrap();
    std::fs::write(root.join("中华书局/论语译注.pdf"), pdf_bytes()).unwrap();

    let output = Command::new(binary())
        .args([
            "sources",
            "add",
            "--name",
            "fixture",
            "--kind",
            "local",
            "--location",
            root.to_str().unwrap(),
        ])
        .env("DATA_DIR", dir.path().join("data"))
        .env("AUTH_USER", "reader")
        .env("AUTH_PASSWORD_HASH", "dummy")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"new\": 1"), "stdout: {stdout}");

    let output = Command::new(binary())
        .args(["sources", "list"])
        .env("DATA_DIR", dir.path().join("data"))
        .env("AUTH_USER", "reader")
        .env("AUTH_PASSWORD_HASH", "dummy")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("fixture"));
    assert!(stdout.contains(root.to_str().unwrap()));

    let output = Command::new(binary())
        .arg("scan")
        .env("DATA_DIR", dir.path().join("data"))
        .env("AUTH_USER", "reader")
        .env("AUTH_PASSWORD_HASH", "dummy")
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\"skipped\": 1"), "stdout: {stdout}");
}
