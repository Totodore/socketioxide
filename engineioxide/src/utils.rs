use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use snowflake::SnowflakeIdGenerator;

pub trait Generator: Clone + Sync + Send + 'static + Debug {
    type Sid: Clone + Hash + Eq + Debug + Display + FromStr + Send + Sync + 'static;
    fn generate_sid(&self) -> Self::Sid;
}

#[derive(Debug)]
pub struct SnowflakeGenerator {
    inner: Arc<Mutex<SnowflakeIdGenerator>>,
}

impl Default for SnowflakeGenerator {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(SnowflakeIdGenerator::new(1, 1))),
        }
    }
}

impl Clone for SnowflakeGenerator {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl Generator for SnowflakeGenerator {
    type Sid = i64;

    fn generate_sid(&self) -> Self::Sid {
        let id = self.inner.lock().unwrap().real_time_generate();
        tracing::debug!("Generating new sid: {}", &id);
        id
    }
}

// pub fn generate_sid() -> i64 {
//     lazy_static! {
//         static ref ID_GENERATOR: Mutex<SnowflakeIdGenerator> =
//             Mutex::new(SnowflakeIdGenerator::new(1, 1));
//     }
//     let id = ID_GENERATOR.lock().unwrap().real_time_generate();
// 	tracing::debug!("Generating new sid: {}", id);
// 	id
// }

#[test]
fn test_generate_sid() {
    let id = generate_sid();
    let id2 = generate_sid();
    assert!(id != id2);
}
