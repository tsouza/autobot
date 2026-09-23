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
fn github_token_wins_over_gh_token_and_cli() {
    let t = resolve_token(env(&[("GITHUB_TOKEN", "a"), ("GH_TOKEN", "b")]), |_| {
        panic!("cli used")
    })
    .unwrap();
    assert_eq!(t, "a");
}

#[test]
fn empty_github_token_falls_through_to_gh_token() {
    let t = resolve_token(env(&[("GITHUB_TOKEN", " "), ("GH_TOKEN", "b")]), |_| {
        panic!("cli used")
    })
    .unwrap();
    assert_eq!(t, "b");
}

#[test]
fn cli_is_named_by_autobot_gh_cli_and_defaults_to_gh() {
    let t = resolve_token(env(&[("AUTOBOT_GH_CLI", "my-gh")]), |cli| {
        Ok(format!("tok-{cli}\n"))
    })
    .unwrap();
    assert_eq!(t, "tok-my-gh");
    let t = resolve_token(env(&[]), |cli| Ok(format!("tok-{cli}"))).unwrap();
    assert_eq!(t, "tok-gh");
}

#[test]
fn empty_cli_token_is_an_error() {
    assert!(resolve_token(env(&[]), |_| Ok("\n".to_owned())).is_err());
}
