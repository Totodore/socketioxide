use std::sync::Mutex;

use lazy_static::lazy_static;
use snowflake::SnowflakeIdGenerator;

pub fn generate_sid() -> i64 {
    lazy_static! {
        static ref ID_GENERATOR: Mutex<SnowflakeIdGenerator> =
            Mutex::new(SnowflakeIdGenerator::new(1, 1));
    }
    let id = ID_GENERATOR.lock().unwrap().real_time_generate();
	tracing::debug!("Generating new sid: {}", id);
	id
}

#[test]
fn test_generate_sid() {
    let id = generate_sid();
    let id2 = generate_sid();
    assert!(id != id2);
}