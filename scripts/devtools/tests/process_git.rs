use autobot_devtools::{git, process::Cmd};

#[test]
fn output_captures_stdout_and_failure_carries_command() {
    assert_eq!(Cmd::new("echo").args(["hi"]).output().unwrap(), "hi\n");
    let err = Cmd::new("false").output().unwrap_err().to_string();
    assert!(err.contains("`false` failed"), "{err}");
}

#[test]
fn toplevel_finds_the_repository() {
    let top = git::toplevel(env!("CARGO_MANIFEST_DIR")).unwrap();
    assert!(std::path::Path::new(&top).join("Cargo.toml").exists());
}
