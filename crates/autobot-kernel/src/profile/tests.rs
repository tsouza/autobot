use super::*;
use std::collections::BTreeSet;

const M0: &str = include_str!("../../../../profiles/m0.toml");

fn m0() -> Profile {
    match Profile::parse(M0) {
        Ok(p) => p,
        Err(e) => panic!("profiles/m0.toml does not parse: {e}"),
    }
}

/// `M0` with the first occurrence of `from` replaced by `to`.
fn edited(from: &str, to: &str) -> String {
    assert!(M0.contains(from), "profiles/m0.toml has no `{from}`");
    M0.replacen(from, to, 1)
}

#[test]
fn parses_the_m0_profile_into_its_values() {
    let v = m0().values().clone();
    assert_eq!(v.version, SCHEMA_VERSION);
    assert_eq!(v.replay.window_days.get(), 30);
    assert_eq!(v.replay.margin_days, 7);
    assert_eq!(v.dispatch_ledger.entries.get(), 64);
    assert_eq!(v.objects.effect_intents_per_command.get(), 8);
    assert_eq!(v.evidence.defect_maturity_days.reversible.get(), 14);
    assert_eq!(
        v.api.reserved_control,
        BTreeSet::from([
            ControlWork::Hold,
            ControlWork::Fence,
            ControlWork::ReceiptRepair
        ])
    );
    assert_eq!(v.sandbox.network, SandboxNetwork::DefaultDeny);
    assert_eq!(v.sandbox.model_api, SandboxModelApi::MeteredProxy);
    assert!(!v.sandbox.host_mounts);
}

#[test]
fn values_round_trip_through_toml_text() {
    let profile = m0();
    let text = match toml::to_string(profile.values()) {
        Ok(t) => t,
        Err(e) => panic!("serializing the values failed: {e}"),
    };
    assert_eq!(Profile::parse(&text), Ok(profile));
}

#[test]
fn digest_ignores_key_order_layout_and_comments() {
    let (head, sandbox) = M0.split_at(M0.find("[sandbox]").expect("m0.toml has a sandbox table"));
    let mut sandbox: Vec<&str> = sandbox.lines().collect();
    sandbox[1..].reverse();
    let head = head.replacen("version = 1\n", "", 1).replace(
        "[replay]\nwindow_days = 30\nmargin_days = 7",
        "# a comment\n[replay]\nmargin_days   =   7\nwindow_days = 30",
    );
    assert!(head.contains("# a comment") && !head.contains("version"));
    let reordered = format!("version = 1\n{}\n\n{head}", sandbox.join("\n"));
    let reordered = Profile::parse(&reordered);
    assert_eq!(reordered.as_ref().map(Profile::digest), Ok(m0().digest()));
}

#[test]
fn digest_is_the_pinned_value_of_the_m0_profile() {
    // Pinned so that a change to the canonical form, which would invalidate every gate's
    // evidence, is a visible edit here rather than a silent side effect.
    assert_eq!(
        m0().digest().to_string(),
        "sha256:22c1b41f59a6de376d9ad5f055c050713307e6f5c92e49c09bc3c7115c0ff84c"
    );
}

#[test]
fn digest_changes_with_every_kind_of_value() {
    let base = m0().digest();
    for (from, to) in [
        ("entries = 64", "entries = 65"),
        ("margin_days = 7", "margin_days = 0"),
        ("host_mounts = false", "host_mounts = true"),
        ("model_api = \"metered-proxy\"", "model_api = \"disabled\""),
        (
            "[\"hold\", \"fence\", \"receipt-repair\"]",
            "[\"hold\", \"fence\"]",
        ),
        (
            "security_or_data_integrity = 30",
            "security_or_data_integrity = 31",
        ),
    ] {
        let changed = Profile::parse(&edited(from, to));
        assert!(
            changed.as_ref().is_ok_and(|p| p.digest() != base),
            "`{from}` -> `{to}`: {changed:?}"
        );
    }
}

#[test]
fn canonical_form_lists_every_leaf_once_in_sorted_order() {
    let table = match toml::Table::try_from(m0().values()) {
        Ok(t) => t,
        Err(e) => panic!("{e}"),
    };
    let form = digest::canonical_form(&table);
    let lines: Vec<&str> = form.lines().collect();
    let mut sorted = lines.clone();
    sorted.sort_unstable();
    assert_eq!(lines, sorted);
    assert_eq!(lines.first(), Some(&"api.burst = 20"));
    assert!(lines.contains(&"api.reserved_control = [\"hold\", \"fence\", \"receipt-repair\"]"));
    assert!(lines.contains(&"evidence.defect_maturity_days.reversible = 14"));
    assert!(lines.contains(&"sandbox.model_api = \"metered-proxy\""));
    assert!(lines.contains(&"version = 1"));
    let leaves = M0
        .lines()
        .filter(|l| l.contains(" = ") && !l.starts_with('#'))
        .count();
    assert_eq!(lines.len(), leaves);
}

#[test]
fn canonical_strings_escape_quotes_backslashes_and_control_characters() {
    let mut table = toml::Table::new();
    table.insert("k".to_owned(), toml::Value::String("a\"b\\c\nd".to_owned()));
    assert_eq!(
        digest::canonical_form(&table),
        "k = \"a\\\"b\\\\c\\u000Ad\"\n"
    );
}

#[test]
fn refuses_documents_that_do_not_match_the_schema() {
    for (from, to) in [
        ("version = 1", "version = 1\nextra = 1"),
        ("entries = 64", "entries = 0"),
        ("entries = 64", "entries = -1"),
        ("entries = 64", "entries = \"64\""),
        ("tasks = 100\n", ""),
        ("os = \"linux\"", "os = \"windows\""),
        ("[\"hold\",", "[\"drain\","),
        ("[registers]", "[registers"),
        ("[\"hold\", \"fence\", \"receipt-repair\"]", "[]"),
        (
            "[\"hold\", \"fence\", \"receipt-repair\"]",
            "[\"hold\", \"hold\"]",
        ),
    ] {
        let result = Profile::parse(&edited(from, to));
        assert!(
            matches!(result, Err(ProfileError::Toml(_))),
            "`{from}` -> `{to}`: {result:?}"
        );
    }
}

#[test]
fn refuses_an_unsupported_schema_version() {
    assert_eq!(
        Profile::parse(&edited("version = 1", "version = 2")),
        Err(ProfileError::Version(2))
    );
}

#[test]
fn refuses_a_pending_slot_larger_than_the_status() {
    let result = Profile::parse(&edited(
        "pending_slot_max_kib = 32",
        "pending_slot_max_kib = 257",
    ));
    assert!(
        matches!(result, Err(ProfileError::Invalid(_))),
        "{result:?}"
    );
    let equal = Profile::parse(&edited(
        "pending_slot_max_kib = 32",
        "pending_slot_max_kib = 256",
    ));
    assert!(equal.is_ok(), "{equal:?}");
}

#[test]
fn digest_text_form_round_trips_and_rejects_malformed_text() {
    let d = m0().digest();
    let text = d.to_string();
    assert_eq!(text.parse::<ProfileDigest>(), Ok(d));
    let upper = format!("sha256:{}", text[7..].to_uppercase());
    for bad in [
        &text[7..],
        &text[..text.len() - 1],
        &format!("{text}0"),
        &upper,
        &format!("sha512:{}", &text[7..]),
        &format!("sha256:{}g", &text[7..text.len() - 1]),
    ] {
        assert!(bad.parse::<ProfileDigest>().is_err(), "accepted `{bad}`");
    }
    let table = toml::Table::from_iter([("d".to_owned(), toml::Value::String(text.clone()))]);
    let back: Result<std::collections::BTreeMap<String, ProfileDigest>, _> = table.try_into();
    assert_eq!(back.ok().and_then(|m| m.get("d").copied()), Some(d));
}

#[test]
fn json_schema_of_the_digest_is_a_patterned_string() {
    let schema = schemars::schema_for!(ProfileDigest);
    assert_eq!(schema.get("type"), Some(&"string".into()));
    assert_eq!(schema.get("pattern"), Some(&"^sha256:[0-9a-f]{64}$".into()));
}

#[test]
fn sandbox_json_schema_is_structural() {
    let schema = schemars::schema_for!(Sandbox);
    let json = schema.as_value().to_string();
    assert!(!json.contains("additionalProperties"), "{json}");
    let properties = schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|o| o.keys().count());
    assert_eq!(properties, Some(9), "{json}");
}

#[test]
fn sandbox_parsing_still_refuses_unknown_keys() {
    let result = Profile::parse(&edited("devices = false", "devices = false\ngpus = 1"));
    assert!(matches!(result, Err(ProfileError::Toml(_))), "{result:?}");
}
