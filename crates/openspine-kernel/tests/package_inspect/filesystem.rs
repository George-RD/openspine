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
    fs::write(fixture.source.join("install.sh"), "#!/bin/sh\necho never-run\n").unwrap();
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
    fs::write(fixture.source.join("NOTE.md"), "two").unwrap();
    fixture.rejected("path-invalid");
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
    fs::write(fixture.source.join("\u{1b}[2JPUBLISHER-VERIFIED.md"), "hidden").unwrap();
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
    fs::hard_link(fixture.source.join("README.md"), fixture.source.join("linked.md")).unwrap();
    fixture.rejected("source-unavailable");
}
