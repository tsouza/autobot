//! Enables GitHub auto-merge, with the squash method, on one pull request.
//!
//! The pull request is named by its number or by its GraphQL node id (the `node_id` of the
//! REST resource). A number is resolved to the node id in the repository [`super::repository`]
//! names. Auto-merge is armed through the `enablePullRequestAutoMerge` mutation, which never
//! merges by itself: GitHub merges the pull request once every required check passes, and the
//! mutation fails when there is nothing left to wait for.

use super::{Client, GraphQl};
use crate::{Error, Result};
use serde_json::json;

/// Arms auto-merge with the squash method and selects the pull request's number.
pub const ENABLE_AUTO_MERGE: &str = "mutation($id: ID!) { enablePullRequestAutoMerge(input: \
    {pullRequestId: $id, mergeMethod: SQUASH}) { pullRequest { number } } }";

/// Looks up the node id of a pull request by repository and number.
pub const PULL_REQUEST_ID: &str = "query($owner: String!, $name: String!, $number: Int!) { \
    repository(owner: $owner, name: $name) { pullRequest(number: $number) { id number } } }";

/// How a pull request is named on the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullRequest {
    /// Its number in the repository.
    Number(u64),
    /// Its GraphQL node id.
    NodeId(String),
}

impl PullRequest {
    /// Parses an argument: all digits is a number; any other non-empty run of ASCII letters,
    /// digits, `_`, `-` and `=` is a node id.
    ///
    /// # Errors
    /// Fails on an empty argument or one with any other character.
    pub fn parse(arg: &str) -> Result<Self> {
        if !arg.is_empty() && arg.bytes().all(|b| b.is_ascii_digit()) {
            return arg
                .parse()
                .map(Self::Number)
                .map_err(|_| Error::Parse(format!("pull request number out of range: `{arg}`")));
        }
        let node_id = |b: u8| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'=');
        if !arg.is_empty() && arg.bytes().all(node_id) {
            return Ok(Self::NodeId(arg.to_owned()));
        }
        Err(Error::Parse(format!(
            "not a pull request number or node id: `{arg}`"
        )))
    }
}

/// The node id of `pr`, looked up in `repo` (`owner/name`) when `pr` is a number.
///
/// # Errors
/// Fails if `repo` is not `owner/name`, the GraphQL call fails, or the repository has no such
/// pull request.
pub fn node_id(api: &impl GraphQl, repo: &str, pr: &PullRequest) -> Result<String> {
    let number = match pr {
        PullRequest::NodeId(id) => return Ok(id.clone()),
        PullRequest::Number(number) => *number,
    };
    let Some((owner, name)) = repo.split_once('/') else {
        return Err(Error::Parse(format!(
            "not an `owner/name` repository: `{repo}`"
        )));
    };
    let data = api.graphql(
        PULL_REQUEST_ID,
        &json!({ "owner": owner, "name": name, "number": number }),
    )?;
    data["repository"]["pullRequest"]["id"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::Parse(format!("{repo}#{number}: no pull request id")))
}

/// Enables auto-merge with the squash method on the pull request with node id `id` and
/// returns its number.
///
/// # Errors
/// Fails if the GraphQL call fails, including when GitHub refuses to arm auto-merge, or the
/// response lacks the pull request's number.
pub fn enable_auto_merge(api: &impl GraphQl, id: &str) -> Result<u64> {
    let data = api.graphql(ENABLE_AUTO_MERGE, &json!({ "id": id }))?;
    data["enablePullRequestAutoMerge"]["pullRequest"]["number"]
        .as_u64()
        .ok_or_else(|| Error::Parse(format!("{id}: no pull request number in the response")))
}

/// Entry point of `scripts/dependabot_automerge.rs`: enables auto-merge (squash) on the pull
/// request named by the only argument, a number or a node id, and prints its number.
///
/// # Errors
/// Fails on a missing, extra or malformed argument, an unresolvable repository, or a failed
/// GitHub call.
pub fn main(args: impl IntoIterator<Item = String>) -> Result<()> {
    let pr = parse_args(args)?;
    let repo = super::repository(".")?;
    let client = Client::new(repo.clone())?;
    let id = node_id(&client, &repo, &pr)?;
    let number = enable_auto_merge(&client, &id)?;
    println!("auto-merge (squash) enabled on {repo}#{number}");
    Ok(())
}

/// The pull request, the only argument.
fn parse_args(args: impl IntoIterator<Item = String>) -> Result<PullRequest> {
    let mut args = args.into_iter();
    match (args.next(), args.next()) {
        (Some(pr), None) => PullRequest::parse(&pr),
        _ => Err(Error::Parse(
            "usage: dependabot_automerge <pull-request-number-or-node-id>".to_owned(),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::graphql_data;
    use super::super::tests::{FOUND, NOT_FOUND};
    use super::*;
    use serde_json::Value;
    use std::cell::RefCell;

    const ID: &str = "PR_kwDOUmSub88AAAABEq1tqw";

    // Recorded from `ENABLE_AUTO_MERGE` with the node id of pull request 236. The pull request
    // was already merged, so GitHub answered with success and changed nothing.
    const ENABLED: &str =
        r#"{"data":{"enablePullRequestAutoMerge":{"pullRequest":{"number":236}}}}"#;

    // Recorded from `ENABLE_AUTO_MERGE` with an id that names no node: GitHub refuses the
    // mutation with a GraphQL error.
    const UNKNOWN_ID: &str = r#"{"data":{"enablePullRequestAutoMerge":null},"errors":[{"type":"NOT_FOUND","path":["enablePullRequestAutoMerge"],"locations":[{"line":1,"column":22}],"message":"Could not resolve to a node with the global id of 'PR_kwDOP_placeholder'"}]}"#;

    /// Answers every call with one recorded response, passed through [`graphql_data`] as
    /// [`Client::graphql`] does, and records each query with its variables.
    struct Recorded {
        response: &'static str,
        calls: RefCell<Vec<(String, Value)>>,
    }

    impl Recorded {
        fn new(response: &'static str) -> Self {
            Self {
                response,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl GraphQl for Recorded {
        fn graphql(&self, query: &str, variables: &Value) -> Result<Value> {
            self.calls
                .borrow_mut()
                .push((query.to_owned(), variables.clone()));
            graphql_data(serde_json::from_str(self.response).unwrap())
        }
    }

    #[test]
    fn enable_auto_merge_sends_the_squash_mutation_for_the_node_id() {
        let api = Recorded::new(ENABLED);
        assert_eq!(enable_auto_merge(&api, ID).unwrap(), 236);
        let calls = api.calls.borrow();
        let [(query, variables)] = calls.as_slice() else {
            panic!("expected one call, got {calls:?}");
        };
        assert_eq!(*variables, json!({ "id": ID }));
        assert!(
            query.starts_with("mutation($id: ID!) { enablePullRequestAutoMerge("),
            "{query}"
        );
        assert!(
            query.contains("{pullRequestId: $id, mergeMethod: SQUASH}"),
            "{query}"
        );
    }

    #[test]
    fn enable_auto_merge_reports_the_graphql_error() {
        let err =
            enable_auto_merge(&Recorded::new(UNKNOWN_ID), "PR_kwDOP_placeholder").unwrap_err();
        assert_eq!(
            err.to_string(),
            "GitHub API error: GraphQL: Could not resolve to a node with the global id of \
             'PR_kwDOP_placeholder'"
        );
    }

    #[test]
    fn enable_auto_merge_requires_the_pull_request_number() {
        let err = enable_auto_merge(&Recorded::new(FOUND), ID).unwrap_err();
        assert!(err.to_string().contains("no pull request number"), "{err}");
    }

    #[test]
    fn a_number_is_resolved_to_its_node_id() {
        let api = Recorded::new(FOUND);
        let id = node_id(&api, "tsouza/autobot", &PullRequest::Number(236)).unwrap();
        assert_eq!(id, ID);
        assert_eq!(
            *api.calls.borrow(),
            [(
                PULL_REQUEST_ID.to_owned(),
                json!({ "owner": "tsouza", "name": "autobot", "number": 236 })
            )]
        );
    }

    #[test]
    fn a_node_id_is_used_as_is_without_a_call() {
        let api = Recorded::new(NOT_FOUND);
        let pr = PullRequest::NodeId(ID.to_owned());
        assert_eq!(node_id(&api, "tsouza/autobot", &pr).unwrap(), ID);
        assert!(api.calls.borrow().is_empty());
    }

    #[test]
    fn an_unknown_number_or_a_bad_repository_is_an_error() {
        let api = Recorded::new(NOT_FOUND);
        let err = node_id(&api, "tsouza/autobot", &PullRequest::Number(99999)).unwrap_err();
        assert!(err.to_string().contains("number of 99999"), "{err}");
        let err = node_id(&api, "autobot", &PullRequest::Number(1)).unwrap_err();
        assert!(err.to_string().contains("owner/name"), "{err}");
    }

    #[test]
    fn the_only_argument_is_a_number_or_a_node_id() {
        assert_eq!(
            parse_args(["236".to_owned()]).unwrap(),
            PullRequest::Number(236)
        );
        assert_eq!(
            parse_args([ID.to_owned()]).unwrap(),
            PullRequest::NodeId(ID.to_owned())
        );
        for args in [
            vec![],
            vec![String::new()],
            vec!["PR_1 x".to_owned()],
            vec!["PR_1;rm".to_owned()],
            vec!["99999999999999999999999".to_owned()],
            vec!["1".to_owned(), "2".to_owned()],
        ] {
            assert!(parse_args(args.clone()).is_err(), "{args:?}");
        }
    }
}
