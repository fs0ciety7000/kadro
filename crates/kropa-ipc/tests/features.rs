// SPDX-License-Identifier: GPL-3.0-or-later

//! The `test-support` feature must never change what the library does (lot 2.1d, review 2.1a m1).
//!
//! `cargo test --workspace --all-targets` compiles `kropa_ipc` with the feature on (the dev-dependency
//! on itself) and drops the fake application in `target/`. Nothing in the library may depend on it,
//! or a binary built by the CI could differ from the one that ships. The CI release step checks the
//! other side: that a release build has no such feature and no fake binary.

use std::path::Path;

fn visit(dir: &Path, found: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            // `src/bin/` holds the fake application, which only exists with the feature.
            if path.file_name().is_some_and(|n| n == "bin") {
                continue;
            }
            visit(&path, found);
        } else if path.extension().is_some_and(|e| e == "rs") {
            let text = std::fs::read_to_string(&path).unwrap();
            for (number, line) in text.lines().enumerate() {
                let code = line.split("//").next().unwrap_or("");
                if code.contains("test-support") {
                    found.push(format!(
                        "{}:{}: {}",
                        path.display(),
                        number + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
}

#[test]
fn the_library_does_not_depend_on_the_test_support_feature() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    visit(&src, &mut found);
    assert!(
        found.is_empty(),
        "`test-support` is used outside src/bin/ (it would enter CI builds):\n{}",
        found.join("\n")
    );
}

#[test]
fn the_scan_does_find_a_use() {
    // The check above must not be blind: the same scan sees a planted use.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::write(
        tmp.path().join("lib.rs"),
        "#[cfg(feature = \"test-support\")]\nfn x() {}\n",
    )
    .unwrap();
    let mut found = Vec::new();
    visit(tmp.path(), &mut found);
    assert_eq!(found.len(), 1);
    let bin = tmp.path().join("bin");
    std::fs::create_dir(&bin).unwrap();
    std::fs::write(bin.join("fake.rs"), "#[cfg(feature = \"test-support\")]\n").unwrap();
    let mut found = Vec::new();
    visit(tmp.path(), &mut found);
    assert_eq!(found.len(), 1, "src/bin/ is exempt");
}
