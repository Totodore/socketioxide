//! [`Extensions`] used to store extra data in each socket instance.
//!
//! It is heavily inspired by the [`http::Extensions`] type from the `http` crate.
//!
//! The main difference is that it uses a [`DashMap`] instead of a [`HashMap`](std::collections::HashMap) to allow concurrent access.
//!
//! This is necessary because [`Extensions`] are shared between all the threads that handle the same socket.

use dashmap::DashMap;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::{
    any::{Any, TypeId},
    hash::{BuildHasherDefault, Hasher},
};

/// TypeMap value
type AnyVal = Box<dyn Any + Send + Sync>;

/// The `AnyDashMap` is a `DashMap` that uses `TypeId` as keys and `Any` as values.
type AnyDashMap = DashMap<TypeId, AnyVal, BuildHasherDefault<IdHasher>>;

/// A wrapper for a `MappedRef` that implements `Deref` and `DerefMut` to allow
/// easy access to the value.
pub struct Ref<'a, T>(
    dashmap::mapref::one::MappedRef<'a, TypeId, AnyVal, T, BuildHasherDefault<IdHasher>>,
);

impl<'a, T> Ref<'a, T> {
    /// Get a reference to the value.
    pub fn value(&self) -> &T {
        self.0.value()
    }
}
impl<'a, T> Deref for Ref<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value()
    }
}

/// A wrapper for a `MappedRefMut` that implements `Deref` and `DerefMut` to allow
/// easy access to the value.
pub struct RefMut<'a, T>(
    dashmap::mapref::one::MappedRefMut<'a, TypeId, AnyVal, T, BuildHasherDefault<IdHasher>>,
);

impl<'a, T> RefMut<'a, T> {
    /// Get a reference to the value.
    pub fn value(&self) -> &T {
        self.0.value()
    }
    /// Get a mutable reference to the value.
    pub fn value_mut(&mut self) -> &mut T {
        self.0.value_mut()
    }
}
impl<'a, T> Deref for RefMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.value()
    }
}
impl<'a, T> DerefMut for RefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.value_mut()
    }
}

impl<'a, T: fmt::Debug> fmt::Debug for Ref<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value().fmt(f)
    }
}
impl<'a, T: fmt::Debug> fmt::Debug for RefMut<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value().fmt(f)
    }
}
impl<'a, T: fmt::Display> fmt::Display for Ref<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value().fmt(f)
    }
}
impl<'a, T: fmt::Display> fmt::Display for RefMut<'a, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.value().fmt(f)
    }
}

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
/// The main difference is that it uses a `DashMap` instead of a `HashMap` to allow concurrent access.
///
/// This is necessary because `Extensions` are shared between all the threads that handle the same socket.
#[derive(Default)]
pub struct Extensions {
    /// The underlying map. It is not wrapped with an option because it would require insert calls to take a mutable reference.
    /// Therefore an anydashmap will be allocated for every socket, even if it is not used.
    map: AnyDashMap,
}

impl Extensions {
    /// Create an empty `Extensions`.
    #[inline]
    pub fn new() -> Extensions {
        Extensions {
            map: AnyDashMap::default(),
        }
    }

    /// Insert a type into this `Extensions`.
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
    pub fn insert<T: Send + Sync + 'static>(&self, val: T) -> Option<T> {
        self.map
            .insert(TypeId::of::<T>(), Box::new(val))
            .and_then(|boxed| {
                (boxed as Box<dyn Any + 'static>)
                    .downcast()
                    .ok()
                    .map(|boxed| *boxed)
            })
    }

    /// Get a reference to a type previously inserted on this `Extensions`.
    ///
    /// # Example
    ///
    /// ```
    /// # use socketioxide::extensions::Extensions;
    /// let ext = Extensions::new();
    /// assert!(ext.get::<i32>().is_none());
    /// ext.insert(5i32);
    ///
    /// assert_eq!(*ext.get::<i32>().unwrap(), 5i32);
    /// ```
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Ref<'_, T>> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|entry| entry.try_map(|r| r.downcast_ref::<T>()).ok())
            .map(|r| Ref(r))
    }

    /// Get a mutable reference to a type previously inserted on this `Extensions`.
    ///
    /// # Example
    ///
    /// ```
    /// # use socketioxide::extensions::Extensions;
    /// let ext = Extensions::new();
    /// ext.insert(String::from("Hello"));
    /// ext.get_mut::<String>().unwrap().push_str(" World");
    ///
    /// assert_eq!(*ext.get::<String>().unwrap(), "Hello World");
    /// ```
    pub fn get_mut<T: Send + Sync + 'static>(&self) -> Option<RefMut<'_, T>> {
        self.map
            .get_mut(&TypeId::of::<T>())
            .and_then(|entry| entry.try_map(|r| r.downcast_mut::<T>()).ok())
            .map(|r| RefMut(r))
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
        self.map.remove(&TypeId::of::<T>()).and_then(|(_, boxed)| {
            (boxed as Box<dyn Any + 'static>)
                .downcast()
                .ok()
                .map(|boxed| *boxed)
        })
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
        self.map.clear();
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
        self.map.is_empty()
    }

    /// Get the numer of extensions available.
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
        self.map.len()
    }
}

impl fmt::Debug for Extensions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Extensions").finish()
    }
}

#[test]
fn test_extensions() {
    #[derive(Debug, PartialEq)]
    struct MyType(i32);

    let extensions = Extensions::new();

    extensions.insert(5i32);
    extensions.insert(MyType(10));

    assert_eq!(extensions.get().as_deref(), Some(&5i32));
    assert_eq!(extensions.get_mut().as_deref_mut(), Some(&mut 5i32));

    assert_eq!(extensions.remove::<i32>(), Some(5i32));
    assert!(extensions.get::<i32>().is_none());

    assert!(extensions.get::<bool>().is_none());
    assert_eq!(extensions.get().as_deref(), Some(&MyType(10)));
}
