use autobot_devtools::cargo::{parse_members, workspace_members};

#[test]
fn parses_normal_dependencies_only() {
    let json = r#"{"packages":[{"name":"a","manifest_path":"/w/a/Cargo.toml","dependencies":[
        {"name":"b","kind":null},{"name":"c","kind":"dev"},{"name":"d","kind":"build"}]}]}"#;
    let members = parse_members(json).unwrap();
    assert_eq!(members[0].name, "a");
    assert_eq!(members[0].normal_dependencies, vec!["b".to_owned()]);
}

#[test]
fn rejects_non_metadata_json() {
    assert!(parse_members("{}").is_err());
    assert!(parse_members("not json").is_err());
}

#[test]
fn reads_this_workspace() {
    let members = workspace_members(env!("CARGO_MANIFEST_DIR")).unwrap();
    assert!(members.iter().any(|p| p.name == "autobot-devtools"));
    assert!(members.iter().any(|p| p.name == "autobot-kernel"));
}
