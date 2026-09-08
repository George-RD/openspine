use super::*;

#[test]
fn package_inspect_nested_artifact_directories_fail() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.source.join("agents/nested")).unwrap();
    fixture.rejected("payload-unsupported");
}

#[test]
fn package_inspect_base_personas_fail() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.source.join("personas")).unwrap();
    fs::write(fixture.source.join("personas/hidden.yaml"), "id: hidden\n").unwrap();
    fixture.rejected("payload-unsupported");
}

#[test]
fn package_inspect_executable_payload_fails() {
    let fixture = Fixture::new();
    fs::write(
        fixture.source.join("install.sh"),
        "#!/bin/sh\necho never-run\n",
    )
    .unwrap();
    fixture.rejected("payload-unsupported");
}

#[test]
fn package_inspect_file_size_limit_fails() {
    let fixture = Fixture::new();
    let file = fs::File::create(fixture.source.join("huge.md")).unwrap();
    file.set_len(8 * 1024 * 1024 + 1).unwrap();
    fixture.rejected("limit-exceeded");
}

#[test]
fn package_inspect_total_size_limit_fails() {
    let fixture = Fixture::new();
    let bytes = vec![b'x'; 8 * 1024 * 1024];
    for index in 0..9 {
        fs::write(fixture.source.join(format!("large-{index}.md")), &bytes).unwrap();
    }
    fixture.rejected("limit-exceeded");
}

#[test]
fn package_inspect_file_count_limit_fails() {
    let fixture = Fixture::new();
    for index in 0..4097 {
        fs::write(fixture.source.join(format!("note-{index}.md")), b"").unwrap();
    }
    fixture.rejected("limit-exceeded");
}

#[test]
fn package_inspect_case_ambiguous_paths_fail() {
    let fixture = Fixture::new();
    fs::write(fixture.source.join("note.md"), "one").unwrap();
    let before = fixture.report();
    // A case-insensitive filesystem cannot represent both directory entries.
    // Exclusive creation must not silently replace the first test fixture.
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(fixture.source.join("NOTE.md"))
    {
        Ok(file) => {
            drop(file);
            fs::write(fixture.source.join("NOTE.md"), "two").unwrap();
            fixture.rejected("path-invalid");
        }
        Err(error) => {
            assert_eq!(error.kind(), std::io::ErrorKind::AlreadyExists);
            assert_eq!(fs::read(fixture.source.join("note.md")).unwrap(), b"one");
            assert_eq!(fixture.report(), before);
        }
    }
}

#[cfg(unix)]
#[test]
fn package_inspect_symlink_file_fails() {
    let fixture = Fixture::new();
    std::os::unix::fs::symlink("README.md", fixture.source.join("link.md")).unwrap();
    fixture.rejected("source-unavailable");
}

#[cfg(unix)]
#[test]
fn package_inspect_symlink_directory_fails() {
    let fixture = Fixture::new();
    let agents = fixture.source.join("agents");
    let outside = fixture.root.path().join("outside-agents");
    fs::rename(&agents, &outside).unwrap();
    std::os::unix::fs::symlink(outside, agents).unwrap();
    fixture.rejected("source-unavailable");
}

#[cfg(unix)]
#[test]
fn package_inspect_socket_fails_without_reading_or_blocking() {
    let fixture = Fixture::new();
    let _socket = std::os::unix::net::UnixListener::bind(fixture.source.join("socket.md")).unwrap();
    fixture.rejected("source-unavailable");
}

#[test]
fn package_inspect_path_diagnostics_do_not_echo_control_text() {
    let fixture = Fixture::new();
    fs::write(
        fixture.source.join("\u{1b}[2JPUBLISHER-VERIFIED.md"),
        "hidden",
    )
    .unwrap();
    for json in [true, false] {
        let output = fixture.run(json);
        assert!(!output.status.success());
        assert!(!output.stdout.contains(&0x1b));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PUBLISHER-VERIFIED"));
        assert!(output.stderr.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn package_inspect_root_symlink_fails() {
    let mut fixture = Fixture::new();
    let link = fixture.root.path().join("linked-source");
    std::os::unix::fs::symlink(&fixture.source, &link).unwrap();
    fixture.source = link;
    fixture.rejected("source-unavailable");
}

#[cfg(unix)]
#[test]
fn package_inspect_hard_links_fail() {
    let fixture = Fixture::new();
    fs::hard_link(
        fixture.source.join("README.md"),
        fixture.source.join("linked.md"),
    )
    .unwrap();
    fixture.rejected("source-unavailable");
}

#[cfg(unix)]
#[test]
fn package_inspect_root_symlink_with_trailing_separator_or_dot_fails() {
    for suffix in ["/", "/."] {
        let mut fixture = Fixture::new();
        let link = fixture.root.path().join("linked-source");
        std::os::unix::fs::symlink(&fixture.source, &link).unwrap();
        fixture.source = format!("{}{suffix}", link.display()).into();
        fixture.rejected("source-unavailable");
    }
}

#[cfg(unix)]
#[test]
fn package_inspect_unreadable_source_entries_fail_closed() {
    use std::os::unix::fs::PermissionsExt as _;
    // Root bypasses Unix permission bits; CI runs this as an ordinary user.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    for relative in ["README.md", "agents"] {
        let fixture = Fixture::new();
        let path = fixture.source.join(relative);
        let permissions = fs::metadata(&path).unwrap().permissions();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
        let output = fixture.run(true);
        fs::set_permissions(path, permissions).unwrap();
        assert!(!output.status.success());
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["error"]["code"], "source-unavailable");
    }
}

#[cfg(unix)]
#[test]
fn package_inspect_non_utf8_filename_fails_without_lossy_aliasing() {
    use std::os::unix::ffi::OsStringExt as _;
    let fixture = Fixture::new();
    let before = fixture.report();
    let name = std::ffi::OsString::from_vec(b"invalid-\xff.md".to_vec());
    match fs::write(fixture.source.join(name), "ordinary bytes") {
        Ok(()) => {
            fixture.rejected("path-invalid");
        }
        Err(error) => {
            // APFS can reject the name itself; no lossy replacement is made.
            assert_eq!(error.raw_os_error(), Some(libc::EILSEQ));
            assert_eq!(fixture.report(), before);
        }
    }
}

#[cfg(unix)]
#[test]
fn package_inspect_executable_document_fails() {
    use std::os::unix::fs::PermissionsExt as _;
    let fixture = Fixture::new();
    fs::set_permissions(
        fixture.source.join("README.md"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    fixture.rejected("payload-unsupported");
}

#[test]
fn package_inspect_binary_document_fails() {
    let fixture = Fixture::new();
    fs::write(fixture.source.join("hidden.md"), [0, 0xff]).unwrap();
    fixture.rejected("payload-unsupported");
}

#[test]
fn package_inspect_exact_file_size_limit_is_accepted() {
    let fixture = Fixture::new();
    fs::write(fixture.source.join("large.md"), vec![b'x'; 8 * 1024 * 1024]).unwrap();
    fixture.report();
}

#[test]
fn package_inspect_exact_total_size_limit_is_accepted() {
    let fixture = Fixture::new();
    let current: usize = file_bytes(&fixture.source).values().map(Vec::len).sum();
    let mut remaining = 64 * 1024 * 1024 - current;
    let mut index = 0;
    while remaining > 0 {
        let bytes = remaining.min(8 * 1024 * 1024);
        fs::write(
            fixture.source.join(format!("padding-{index}.md")),
            vec![b'x'; bytes],
        )
        .unwrap();
        remaining -= bytes;
        index += 1;
    }
    let report = fixture.report();
    let total: u64 = report["inventory"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["bytes"].as_u64().unwrap())
        .sum();
    assert_eq!(total, 64 * 1024 * 1024);
}

#[test]
fn package_inspect_exact_file_count_limit_is_accepted() {
    let fixture = Fixture::new();
    let existing = file_bytes(&fixture.source).len();
    for index in existing..4096 {
        fs::write(fixture.source.join(format!("padding-{index}.md")), "").unwrap();
    }
    assert_eq!(
        fixture.report()["inventory"].as_array().unwrap().len(),
        4096
    );
}

#[cfg(unix)]
#[test]
fn package_inspect_operator_parent_alias_does_not_change_content_identity() {
    let mut fixture = Fixture::new();
    let expected = fixture.report();
    let alias = fixture.root.path().join("parent-alias");
    std::os::unix::fs::symlink(fixture.root.path(), &alias).unwrap();
    fixture.source = alias.join("candidate");
    assert_eq!(fixture.report(), expected);
}
