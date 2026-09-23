use super::*;
use std::net::TcpListener;

const ACCESS: &str = "AKIAIOSFODNN7EXAMPLE";
const SECRET: &str = "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY";
const DATE: &str = "20130524T000000Z";
const HOST: &str = "examplebucket.s3.amazonaws.com";

fn signature(
    method: &str,
    path: &str,
    query: &[(&str, &str)],
    extra: &[(&str, &str)],
    payload: &str,
) -> String {
    let mut headers = vec![
        ("host", HOST),
        ("x-amz-content-sha256", payload),
        ("x-amz-date", DATE),
    ];
    headers.extend_from_slice(extra);
    let signer = sigv4::Signer {
        access_key: ACCESS,
        secret_key: SECRET,
        region: "us-east-1",
    };
    let auth = signer.authorization(
        &sigv4::Request {
            method,
            path: &sigv4::encode_path(path),
            query,
            headers: &headers,
            payload_sha256: payload,
        },
        DATE,
    );
    let (credential, signature) = auth.split_once(", Signature=").unwrap();
    assert!(
        credential.starts_with(&format!(
            "AWS4-HMAC-SHA256 Credential={ACCESS}/20130524/us-east-1/s3/aws4_request, SignedHeaders="
        )),
        "{auth}"
    );
    signature.to_owned()
}

// The four signed-header examples of the Amazon S3 API reference, "Signature Calculations for
// the Authorization Header: Transferring Payload in a Single Chunk".
#[test]
fn signatures_match_the_published_s3_examples() {
    assert_eq!(
        signature(
            "GET",
            "/test.txt",
            &[],
            &[("range", "bytes=0-9")],
            EMPTY_SHA256
        ),
        "f0e8bdb87c964420e857bd35b5d6ed310bd44f0170aba48dd91039c6036bdb41"
    );
    let body = b"Welcome to Amazon S3.";
    let payload = sigv4::sha256_hex(body);
    assert_eq!(
        payload,
        "44ce7dd67c959e0d3524ffac1771dfbba87d2b6b4b4e99e42034a8b803f8b072"
    );
    assert_eq!(
        signature(
            "PUT",
            "/test$file.text",
            &[],
            &[
                ("date", "Fri, 24 May 2013 00:00:00 GMT"),
                ("x-amz-storage-class", "REDUCED_REDUNDANCY"),
            ],
            &payload,
        ),
        "98ad721746da40c64f1a55b78f14c238d841ea1380cd77a1b5971af0ece108bd"
    );
    assert_eq!(
        signature("GET", "/", &[("lifecycle", "")], &[], EMPTY_SHA256),
        "fea454ca298b7da1c68078a5d1bdbfbbe0d65c699e0f91ac7a200a0136783543"
    );
    assert_eq!(
        signature(
            "GET",
            "/",
            &[("prefix", "J"), ("max-keys", "2")],
            &[],
            EMPTY_SHA256
        ),
        "34b48302e7b5fa45bde8084f4b7868a86f0a534bc59db6670ed5711ef69dc6f7"
    );
}

#[test]
fn the_empty_payload_hash_is_the_sha256_of_nothing() {
    assert_eq!(sigv4::sha256_hex(b""), EMPTY_SHA256);
}

#[test]
fn dates_are_utc_in_the_amz_format() {
    assert_eq!(sigv4::amz_date(0), "19700101T000000Z");
    assert_eq!(sigv4::amz_date(1_369_353_600), DATE);
    assert_eq!(sigv4::amz_date(951_782_400), "20000229T000000Z");
    assert_eq!(sigv4::amz_date(4_102_444_799), "20991231T235959Z");
}

#[test]
fn paths_and_queries_encode_all_but_unreserved_characters() {
    assert_eq!(sigv4::encode_path("/a b/$+~-._"), "/a%20b/%24%2B~-._");
    assert_eq!(sigv4::encode_path("/é"), "/%C3%A9");
    assert_eq!(
        sigv4::encode_query(&[("prefix", "w/k/"), ("delimiter", "/"), ("versioning", "")]),
        "delimiter=%2F&prefix=w%2Fk%2F&versioning="
    );
}

#[test]
fn xml_leaves_are_read_by_exact_tag_with_entities_resolved() {
    let xml = "<ListBucketResult><KeyCount>2</KeyCount><Contents><Key>a&amp;b</Key></Contents>\
               <Contents><Key id=\"x\">c&#x2F;&#47;&lt;</Key></Contents><Key/>\
               <IsTruncated>false</IsTruncated></ListBucketResult>";
    assert_eq!(xml::texts(xml, "Key"), ["a&b", "c//<"]);
    assert_eq!(xml::text(xml, "KeyCount").as_deref(), Some("2"));
    assert_eq!(xml::text(xml, "IsTruncated").as_deref(), Some("false"));
    assert_eq!(xml::text(xml, "Status"), None);
    assert_eq!(
        xml::texts("<Key>a &bogus; &amp</Key>", "Key"),
        ["a &bogus; &amp"]
    );
}

#[test]
fn only_names_of_this_layout_are_versions() {
    let prefix = "run/w k/manifest/";
    assert_eq!(
        version_of("run/w+k/manifest/00000000000000000007", prefix),
        Some(7)
    );
    assert_eq!(
        version_of("run%2Fw%20k%2Fmanifest%2F18446744073709551615", prefix),
        Some(u64::MAX)
    );
    for listed in [
        "run/w+k/manifest/0000000000000000007",
        "run/w+k/manifest/99999999999999999999",
        "run/w+k/manifest/0000000000000000000x",
        "run/w+k/manifest/00000000000000000007/00000000000000000000",
        "run/w+k/other/00000000000000000007",
        "run/w+k/manifest/%zz",
    ] {
        assert_eq!(version_of(listed, prefix), None, "{listed}");
    }
}

#[test]
fn endpoints_are_a_scheme_and_an_authority() {
    assert_eq!(
        parse_endpoint("https://s3.example.com:443/").unwrap(),
        (
            "https://s3.example.com".to_owned(),
            "s3.example.com".to_owned()
        )
    );
    assert_eq!(
        parse_endpoint("http://127.0.0.1:9000").unwrap(),
        (
            "http://127.0.0.1:9000".to_owned(),
            "127.0.0.1:9000".to_owned()
        )
    );
    for bad in [
        "ftp://x",
        "s3.example.com",
        "http://",
        "http://h/p",
        "http://u@h",
        "http://h?x",
    ] {
        assert!(
            matches!(parse_endpoint(bad), Err(ConnectError::Config(_))),
            "{bad}"
        );
    }
}

fn config(endpoint: &str, prefix: &str) -> S3Config {
    S3Config {
        endpoint: endpoint.to_owned(),
        region: "us-east-1".to_owned(),
        bucket: "artifacts".to_owned(),
        prefix: prefix.to_owned(),
        access_key: "access".to_owned(),
        secret_key: "secret".to_owned(),
        max_bytes: 4,
        timeout: Duration::from_secs(5),
    }
}

#[test]
fn an_unusable_configuration_is_refused_before_any_request() {
    for (bucket, prefix) in [
        ("", ""),
        ("a/b", ""),
        ("artifacts", "/p"),
        ("artifacts", "p/"),
    ] {
        let mut c = config("http://127.0.0.1:1", prefix);
        c.bucket = bucket.to_owned();
        assert!(
            matches!(S3ArtifactStore::connect(c), Err(ConnectError::Config(_))),
            "{bucket:?} {prefix:?}"
        );
    }
}

#[test]
fn the_secret_key_never_prints() {
    let shown = format!("{:?}", config("http://h", ""));
    assert!(!shown.contains("\"secret\""), "{shown}");
    assert!(shown.contains("<redacted>"), "{shown}");
}

/// A client of `endpoint` that has not checked the bucket.
fn unchecked(endpoint: &str, prefix: &str) -> S3ArtifactStore {
    S3ArtifactStore::unchecked(config(endpoint, prefix)).unwrap()
}

#[test]
fn object_names_nest_the_version_under_the_key_and_prefix() {
    let key = ArtifactKey::new("w/manifest").unwrap();
    assert_eq!(
        unchecked("http://h", "run").object_path(&key, 7),
        "/artifacts/run/w/manifest/00000000000000000007"
    );
    assert_eq!(
        unchecked("http://h", "").object_path(&key, u64::MAX),
        "/artifacts/w/manifest/18446744073709551615"
    );
}

#[test]
fn a_put_over_the_bound_sends_nothing() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}", listener.local_addr().unwrap());
    let mut store = unchecked(&endpoint, "");
    let key = ArtifactKey::new("w/manifest").unwrap();
    assert_eq!(store.put(&key, b"12345"), Err(StoreError::Unavailable));
    let accepted = listener.accept();
    assert!(
        accepted
            .as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::WouldBlock),
        "{accepted:?}"
    );
}

#[test]
fn the_bound_is_the_profiles_per_workspace_bound() {
    let profile =
        autobot_kernel::profile::Profile::parse(include_str!("../../../../../profiles/m0.toml"))
            .unwrap();
    let artifacts = &profile.values().artifacts;
    assert_eq!(
        S3Config::bound(artifacts),
        u64::from(artifacts.workspace_max_gib.get()) * 1024 * 1024 * 1024
    );
}

fn answered(status: u16) -> Result<Response<Body>, ureq::Error> {
    Ok(Response::builder()
        .status(status)
        .body(Body::builder().data(Vec::new()))
        .unwrap())
}

#[test]
fn a_put_is_uncertain_once_its_request_may_have_reached_the_store() {
    use std::io::{Error, ErrorKind};
    let unavailable = [
        Err(ureq::Error::ConnectionFailed),
        Err(ureq::Error::HostNotFound),
        Err(ureq::Error::Timeout(ureq::Timeout::Connect)),
        Err(ureq::Error::Io(Error::from(ErrorKind::ConnectionRefused))),
        answered(403),
        answered(400),
        answered(503),
        answered(307),
    ];
    for answer in &unavailable {
        assert_eq!(
            claim(answer),
            Claim::Failed(StoreError::Unavailable),
            "{answer:?}"
        );
    }
    let uncertain = [
        Err(ureq::Error::Timeout(ureq::Timeout::RecvResponse)),
        Err(ureq::Error::Timeout(ureq::Timeout::SendBody)),
        Err(ureq::Error::Io(Error::from(ErrorKind::ConnectionReset))),
        Err(ureq::Error::Io(Error::from(ErrorKind::UnexpectedEof))),
        answered(500),
        answered(502),
    ];
    for answer in &uncertain {
        assert_eq!(
            claim(answer),
            Claim::Failed(StoreError::Uncertain),
            "{answer:?}"
        );
    }
    assert_eq!(claim(&answered(200)), Claim::Stored);
    assert_eq!(claim(&answered(412)), Claim::Taken);
    assert_eq!(claim(&answered(409)), Claim::Taken);
}
