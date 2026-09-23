use super::*;
use autobot_adapters::observation::ForgeText;
use autobot_adapters::text::Actor;

fn observation(body: &str) -> Observation {
    Observation {
        provider: ProviderName::new("p").unwrap(),
        event_id: EventId::new("e").unwrap(),
        semantic_key: Digest::from_bytes([1; 32]),
        object: RemoteIdentity::new("o").unwrap(),
        generation: 1,
        source_head: None,
        base_head: None,
        protection_digest: None,
        observed_at: EPOCH,
        fact: Fact::Text(ForgeText {
            actor: Actor::new("a").unwrap(),
            body: body.to_owned(),
        }),
    }
}

#[test]
fn a_signature_covers_the_content_and_the_key() {
    let signed = Signed::new(observation("approve"), &SIGNING_KEY);
    assert!(signed.verifies());
    let mut altered = signed.clone();
    altered.observation = observation("approve and merge");
    assert!(!altered.verifies());
    assert!(!Signed::new(observation("approve"), &FORGER_KEY).verifies());
}

#[test]
fn a_present_field_never_encodes_like_an_absent_one() {
    let mut with_head = observation("x");
    with_head.base_head = Some(Head::new("h").unwrap());
    assert_ne!(signed_bytes(&with_head), signed_bytes(&observation("x")));
    let mut moved = with_head.clone();
    moved.base_head = None;
    moved.source_head = Some(Head::new("h").unwrap());
    assert_ne!(signed_bytes(&with_head), signed_bytes(&moved));
}

const NO_FAULTS: Delivery = Delivery {
    duplicate: false,
    reorder: false,
    delay_polls: 0,
};

#[test]
fn a_poll_refuses_an_answer_whose_event_id_semantic_key_or_observation_time_was_altered() {
    let alterations: [fn(&mut Observation); 3] = [
        |o| o.event_id = EventId::new("another-event").unwrap(),
        |o| o.semantic_key = Digest::from_bytes([2; 32]),
        |o| o.observed_at += 1,
    ];
    let provider = ProviderName::new("p").unwrap();
    let mut feed = Feed::new(provider.clone(), NO_FAULTS);
    feed.forged
        .push_back(Signed::new(observation("approve"), &SIGNING_KEY));
    assert_eq!(feed.poll(), Ok(vec![observation("approve")]));
    for alter in alterations {
        let mut signed = Signed::new(observation("approve"), &SIGNING_KEY);
        alter(&mut signed.observation);
        let mut feed = Feed::new(provider.clone(), NO_FAULTS);
        feed.forged.push_back(signed);
        assert_eq!(feed.poll(), Err(SourceError::Unauthenticated));
    }
}
