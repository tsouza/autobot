#![cfg(feature = "github")]

use autobot_devtools::github::resolve_token;
use std::collections::HashMap;

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    move |k| map.get(k).cloned()
}

#[test]
fn github_token_wins_over_gh_token() {
    let t = resolve_token(env(&[("GITHUB_TOKEN", "a"), ("GH_TOKEN", "b")])).unwrap();
    assert_eq!(t, "a");
}

#[test]
fn blank_github_token_falls_through_to_gh_token() {
    let t = resolve_token(env(&[("GITHUB_TOKEN", " "), ("GH_TOKEN", "b\n")])).unwrap();
    assert_eq!(t, "b");
}

#[test]
fn missing_tokens_are_an_error() {
    assert!(resolve_token(env(&[])).is_err());
    assert!(resolve_token(env(&[("GITHUB_TOKEN", ""), ("GH_TOKEN", "\n")])).is_err());
}
