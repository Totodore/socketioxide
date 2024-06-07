use std::{
    collections::HashMap,
    sync::{Arc, RwLock},
};

use criterion::{black_box, criterion_group, criterion_main, BatchSize, Criterion};
use matchit::Router;
use rand::Rng;
use slab::Slab;

#[derive(Debug, Default)]
struct NsBuff {
    /// Buffer of all the namespaces that the server is handling
    buff: Slab<Arc<String>>,
    /// Router used to find the namespace for a given path
    /// Map to an index in the [`Slab`]
    router: Router<usize>,
}

struct LegacyNsBuff {
    map: HashMap<String, Arc<String>>,
}

impl NsBuff {
    pub fn get(&self, path: &str) -> Option<Arc<String>> {
        self.router
            .at(path)
            .ok()
            .map(|m| self.buff[*m.value].clone())
    }

    pub fn insert(&mut self, path: Arc<String>) -> Result<(), matchit::InsertError> {
        let index = self.buff.insert(path.clone());
        self.router.insert(path.as_str(), index).map_err(|e| {
            self.buff.remove(index);
            e
        })
    }
}

fn bench_router(c: &mut Criterion) {
    let mut group = c.benchmark_group("router");
    group.bench_function("concurrent_inserts", |b| {
        let buff = RwLock::new(NsBuff::default());
        b.iter_batched(
            || format!("/test/{}", rand::thread_rng().gen_range(0..1000)),
            |p| {
                buff.write().unwrap().insert(black_box(p).into()).ok();
            },
            BatchSize::SmallInput,
        );
    });
    group.bench_function("concurrent_get", |b| {
        let mut buff = NsBuff::default();
        buff.insert(black_box("/user/{id}").to_string().into())
            .unwrap();
        let buff = RwLock::new(buff);
        b.iter(|| {
            buff.read().unwrap().get(black_box("/user/5"));
        })
    });

    group.finish();
}

fn bench_legacy_router(c: &mut Criterion) {
    let mut group = c.benchmark_group("legacy_router");
    group.bench_function("concurrent_inserts", |b| {
        let buff = RwLock::new(LegacyNsBuff {
            map: HashMap::new(),
        });
        b.iter_batched(
            || format!("/test/{}", rand::thread_rng().gen_range(0..1000)),
            |p| {
                buff.write()
                    .unwrap()
                    .map
                    .insert(black_box(p), "test".to_string().into());
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("concurrent_get", |b| {
        let mut buff = LegacyNsBuff {
            map: HashMap::new(),
        };
        buff.map.insert(
            black_box("/user/{id}").to_string(),
            "test".to_string().into(),
        );
        let buff = RwLock::new(buff);
        b.iter(|| {
            buff.read().unwrap().map.get(black_box("/user/5")).cloned();
        })
    });
}

criterion_group!(benches, bench_router, bench_legacy_router);
criterion_main!(benches);
