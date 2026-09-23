use autobot_devtools::{git, process::Cmd};
use std::path::Path;

fn sh(dir: &Path, args: &[&str]) {
    Cmd::new("git")
        .args(args.iter().copied())
        .current_dir(dir)
        .output()
        .unwrap();
}

fn temp_repo() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("devtools-git-{}-{}", std::process::id(), line!()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    sh(&dir, &["init", "-q", "-b", "main"]);
    sh(&dir, &["config", "user.email", "t@example.com"]);
    sh(&dir, &["config", "user.name", "t"]);
    std::fs::write(dir.join("a"), "1").unwrap();
    sh(&dir, &["add", "a"]);
    sh(&dir, &["commit", "-qm", "base"]);
    dir
}

#[test]
fn output_captures_stdout_and_failure_carries_command() {
    assert_eq!(Cmd::new("echo").args(["hi"]).output().unwrap(), "hi\n");
    let err = Cmd::new("false").output().unwrap_err().to_string();
    assert!(err.contains("`false` failed"), "{err}");
}

#[test]
fn run_reports_success_and_failure() {
    assert!(Cmd::new("true").run().is_ok());
    let err = Cmd::new("sh")
        .args(["-c", "exit 3"])
        .run()
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("`sh -c exit 3` failed") && err.contains('3'),
        "{err}"
    );
}

#[test]
fn toplevel_finds_the_repository() {
    let top = git::toplevel(env!("CARGO_MANIFEST_DIR")).unwrap();
    assert!(Path::new(&top).join("Cargo.toml").exists());
}

#[test]
fn merge_base_and_three_dot_changed_paths() {
    let dir = temp_repo();
    let base = Cmd::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&dir)
        .output()
        .unwrap();
    sh(&dir, &["checkout", "-qb", "feature"]);
    std::fs::write(dir.join("b"), "2").unwrap();
    sh(&dir, &["add", "b"]);
    sh(&dir, &["commit", "-qm", "feature"]);
    sh(&dir, &["checkout", "-q", "main"]);
    std::fs::write(dir.join("c"), "3").unwrap();
    sh(&dir, &["add", "c"]);
    sh(&dir, &["commit", "-qm", "main moves"]);
    assert_eq!(
        git::merge_base(&dir, "main", "feature").unwrap(),
        base.trim()
    );
    // Three-dot: only what the feature branch changed since the merge base, not main's `c`.
    assert_eq!(
        git::changed_paths(&dir, "main", "feature").unwrap(),
        vec!["b".to_owned()]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}
