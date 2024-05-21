//! [`Extensions`] used to store extra data in each socket instance.
//!
//! It is heavily inspired by the [`http::Extensions`] type from the `http` crate.
//!
//! The main difference is that the inner [`HashMap`] is wrapped with an [`RwLock`]
//! to allow concurrent access. Moreover, any value extracted from the map is cloned before being returned.
//!
//! This is necessary because [`Extensions`] are shared between all the threads that handle the same socket.
//!
//! You can use the [`Extension`](crate::extract::Extension) or
//! [`MaybeExtension`](crate::extract::MaybeExtension) extractor to extract an extension of the given type.

use std::collections::HashMap;
use std::fmt;
use std::sync::RwLock;
use std::{
    any::{Any, TypeId},
    hash::{BuildHasherDefault, Hasher},
};

/// TypeMap value
type AnyVal = Box<dyn Any + Send + Sync>;

/// The `AnyDashMap` is a `HashMap` that uses `TypeId` as keys and `Any` as values.
type AnyHashMap = RwLock<HashMap<TypeId, AnyVal, BuildHasherDefault<IdHasher>>>;

// With TypeIds as keys, there's no need to hash them. They are already hashes
// themselves, coming from the compiler. The IdHasher just holds the u64 of
// the TypeId, and then returns it, instead of doing any bit fiddling.
#[derive(Default)]
struct IdHasher(u64);

impl Hasher for IdHasher {
    #[inline]
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, _: &[u8]) {
        unreachable!("TypeId calls write_u64");
    }

    #[inline]
    fn write_u64(&mut self, id: u64) {
        self.0 = id;
    }
}

/// A type map of protocol extensions.
///
/// It is heavily inspired by the `Extensions` type from the `http` crate.
///
/// The main difference is that the inner Map is wrapped with an `RwLock` to allow concurrent access.
///
/// This is necessary because `Extensions` are shared between all the threads that handle the same socket.
#[derive(Default)]
pub struct Extensions {
    /// The underlying map
    map: AnyHashMap,
}

impl Extensions {
    /// Create an empty `Extensions`.
    #[inline]
    pub fn new() -> Extensions {
        Extensions {
            map: AnyHashMap::default(),
        }
    }

    /// Insert a type into this `Extensions`.
    ///
    /// The type must be cloneable and thread safe to be stored.
    ///
    /// If a extension of this type already existed, it will
    /// be returned.
    ///
    /// # Example
    ///
    /// ```
    /// # use socketioxide::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// assert!(ext.insert(5i32).is_none());
    /// assert!(ext.insert(4u8).is_none());
    /// assert_eq!(ext.insert(9i32), Some(5i32));
    /// ```
    pub fn insert<T: Send + Sync + Clone + 'static>(&self, val: T) -> Option<T> {
        self.map
            .write()
            .unwrap()
            .insert(TypeId::of::<T>(), Box::new(val))
            .and_then(|v| v.downcast().ok().map(|boxed| *boxed))
    }

    /// Get a cloned value of a type previously inserted on this `Extensions`.
    ///
    /// # Example
    ///
    /// ```
    /// # use socketioxide::extensions::Extensions;
    /// let ext = Extensions::new();
    /// assert!(ext.get::<i32>().is_none());
    /// ext.insert(5i32);
    ///
    /// assert_eq!(ext.get::<i32>().unwrap(), 5i32);
    /// ```
    pub fn get<T: Send + Sync + Clone + 'static>(&self) -> Option<T> {
        self.map
            .read()
            .unwrap()
            .get(&TypeId::of::<T>())
            .and_then(|v| v.downcast_ref::<T>())
            .cloned()
    }

    /// Remove a type from this `Extensions`.
    ///
    /// If a extension of this type existed, it will be returned.
    ///
    /// # Example
    ///
    /// ```
    /// # use socketioxide::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// ext.insert(5i32);
    /// assert_eq!(ext.remove::<i32>(), Some(5i32));
    /// assert!(ext.get::<i32>().is_none());
    /// ```
    pub fn remove<T: Send + Sync + 'static>(&self) -> Option<T> {
        self.map
            .write()
            .unwrap()
            .remove(&TypeId::of::<T>())
            .and_then(|v| v.downcast().ok().map(|boxed| *boxed))
    }

    /// Clear the `Extensions` of all inserted extensions.
    ///
    /// # Example
    ///
    /// ```
    /// # use socketioxide::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// ext.insert(5i32);
    /// ext.clear();
    ///
    /// assert!(ext.get::<i32>().is_none());
    /// ```
    #[inline]
    pub fn clear(&self) {
        self.map.write().unwrap().clear();
    }

    /// Check whether the extension set is empty or not.
    ///
    /// # Example
    ///
    /// ```
    /// # use socketioxide::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// assert!(ext.is_empty());
    /// ext.insert(5i32);
    /// assert!(!ext.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.read().unwrap().is_empty()
    }

    /// Get the number of extensions available.
    ///
    /// # Example
    ///
    /// ```
    /// # use socketioxide::extensions::Extensions;
    /// let mut ext = Extensions::new();
    /// assert_eq!(ext.len(), 0);
    /// ext.insert(5i32);
    /// assert_eq!(ext.len(), 1);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.map.read().unwrap().len()
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions").finish()
    }
}

#[test]
fn test_extensions() {
    use std::sync::Arc;
    #[derive(Debug, Clone, PartialEq)]
    struct MyType(i32);

    #[derive(Debug, PartialEq)]
    struct ComplexSharedType(u64);
    let shared = Arc::new(ComplexSharedType(20));

    let extensions = Extensions::new();

    extensions.insert(5i32);
    extensions.insert(MyType(10));
    extensions.insert(shared.clone());

    assert_eq!(extensions.get(), Some(5i32));
    assert_eq!(extensions.get::<Arc<ComplexSharedType>>(), Some(shared));

    assert_eq!(extensions.remove::<i32>(), Some(5i32));
    assert!(extensions.get::<i32>().is_none());

    assert!(extensions.get::<bool>().is_none());
    assert_eq!(extensions.get(), Some(MyType(10)));
}
