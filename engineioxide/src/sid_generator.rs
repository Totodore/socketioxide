use base64id::Id64;
use rand::Rng;

pub type Sid = Id64;

pub fn generate_sid() -> Sid {
    let id: Id64 = rand::thread_rng().gen();

    tracing::debug!("Generating new sid: {}", id);
    id
}

#[test]
fn test_generate_sid() {
    let id = generate_sid();
    let id2 = generate_sid();
    assert_ne!(id, id2);
}
