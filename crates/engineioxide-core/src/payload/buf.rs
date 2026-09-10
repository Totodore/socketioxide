//! Buffer utilities.

use std::collections::VecDeque;
use std::io::IoSlice;

use bytes::{Buf, BufMut, Bytes, BytesMut};

/// The Buf list is used to store the data from the body of the request in a zero-copy fashion.
/// Each time a new chunk of data is received, it is pushed to the back of the list.
/// It implements the `Buf` trait itself so that it can be used as a
/// `Buf` in the `Payload` struct.
///
/// This implementation is based on the private [`BufList`](https://github.com/hyperium/hyper/blob/d977f209bc6068d8f878b22803fc42d90c887fcc/src/common/buf.rs) mod from the [`hyper`](hyper) crate.
#[derive(Debug)]
pub struct BufList<T> {
    bufs: VecDeque<T>,
    /// Total remaining bytes across `bufs`, so [`Buf::remaining`] is O(1).
    remaining: usize,
}
impl<T> Default for BufList<T> {
    fn default() -> Self {
        Self {
            bufs: VecDeque::new(),
            remaining: 0,
        }
    }
}

impl<T: Buf> BufList<T> {
    /// Create a new empty [`BufList`]
    pub fn new() -> BufList<T> {
        BufList::default()
    }

    /// Push a new buf into the [`BufList`].
    ///
    /// An empty buf is ignored: every queued buf has data, so the front
    /// chunk is never empty while there are remaining bytes.
    #[inline]
    pub fn push(&mut self, buf: T) {
        debug_assert!(buf.has_remaining());
        if !buf.has_remaining() {
            return;
        }
        self.remaining += buf.remaining();
        self.bufs.push_back(buf);
    }
}

impl<T: Buf> Buf for BufList<T> {
    #[inline]
    fn remaining(&self) -> usize {
        self.remaining
    }

    #[inline]
    fn has_remaining(&self) -> bool {
        !self.bufs.is_empty()
    }

    #[inline]
    fn chunk(&self) -> &[u8] {
        self.bufs.front().map(Buf::chunk).unwrap_or_default()
    }

    #[inline]
    fn chunks_vectored<'t>(&'t self, dst: &mut [IoSlice<'t>]) -> usize {
        if dst.is_empty() {
            return 0;
        }
        let mut vecs = 0;
        for buf in &self.bufs {
            vecs += buf.chunks_vectored(&mut dst[vecs..]);
            if vecs == dst.len() {
                break;
            }
        }
        vecs
    }

    #[inline]
    fn advance(&mut self, mut cnt: usize) {
        assert!(cnt <= self.remaining, "`cnt` greater than remaining");
        self.remaining -= cnt;
        while cnt > 0 {
            {
                let front = &mut self.bufs[0];
                let rem = front.remaining();
                if rem > cnt {
                    front.advance(cnt);
                    return;
                } else {
                    front.advance(rem);
                    cnt -= rem;
                }
            }
            self.bufs.pop_front();
        }
    }

    #[inline]
    fn copy_to_bytes(&mut self, len: usize) -> Bytes {
        // Our inner buffer may have an optimized version of copy_to_bytes, and if the whole
        // request can be fulfilled by the front buffer, we can take advantage.
        match self.bufs.front_mut() {
            Some(front) if front.remaining() == len => {
                let b = front.copy_to_bytes(len);
                self.bufs.pop_front();
                self.remaining -= len;
                b
            }
            Some(front) if front.remaining() > len => {
                let b = front.copy_to_bytes(len);
                self.remaining -= len;
                b
            }
            _ => {
                assert!(len <= self.remaining, "`len` greater than remaining");
                let mut bm = BytesMut::with_capacity(len);
                // `take` advances `self`, which keeps `remaining` in sync
                bm.put(self.take(len));
                bm.freeze()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ptr;

    use super::*;

    fn hello_world_buf() -> BufList<Bytes> {
        let mut bufs = BufList::new();
        for chunk in ["Hello", " ", "World"] {
            bufs.push(Bytes::from(chunk));
        }
        bufs
    }

    /// The cached length must match the queued buffers after every
    /// operation.
    #[test]
    fn remaining_is_kept_in_sync() {
        let mut bufs = hello_world_buf();
        let walk = |bufs: &BufList<Bytes>| bufs.bufs.iter().map(Buf::remaining).sum::<usize>();
        assert_eq!(bufs.remaining(), 11);
        assert_eq!(bufs.remaining(), walk(&bufs));

        bufs.advance(3); // inside the first buf
        assert_eq!(bufs.remaining(), 8);
        assert_eq!(bufs.remaining(), walk(&bufs));

        bufs.copy_to_bytes(2); // "lo": exactly the rest of the first buf
        assert_eq!(bufs.remaining(), 6);
        assert_eq!(bufs.remaining(), walk(&bufs));

        bufs.copy_to_bytes(3); // " Wo": spans two bufs
        assert_eq!(bufs.remaining(), 3);
        assert_eq!(bufs.remaining(), walk(&bufs));
        assert_eq!(bufs.chunk(), b"rld");

        bufs.advance(3);
        assert_eq!(bufs.remaining(), 0);
        assert!(!bufs.has_remaining());
        assert!(bufs.bufs.is_empty());
    }

    #[test]
    #[should_panic(expected = "`cnt` greater than remaining")]
    fn advance_too_far_panics() {
        hello_world_buf().advance(12);
    }

    #[test]
    fn to_bytes_shorter() {
        let mut bufs = hello_world_buf();
        let old_ptr = bufs.chunk().as_ptr();
        let start = bufs.copy_to_bytes(4);
        assert_eq!(start, "Hell");
        assert!(ptr::eq(old_ptr, start.as_ptr()));
        assert_eq!(bufs.chunk(), b"o");
        assert!(ptr::eq(old_ptr.wrapping_add(4), bufs.chunk().as_ptr()));
        assert_eq!(bufs.remaining(), 7);
    }

    #[test]
    fn to_bytes_eq() {
        let mut bufs = hello_world_buf();
        let old_ptr = bufs.chunk().as_ptr();
        let start = bufs.copy_to_bytes(5);
        assert_eq!(start, "Hello");
        assert!(ptr::eq(old_ptr, start.as_ptr()));
        assert_eq!(bufs.chunk(), b" ");
        assert_eq!(bufs.remaining(), 6);
    }

    #[test]
    fn to_bytes_longer() {
        let mut bufs = hello_world_buf();
        let start = bufs.copy_to_bytes(7);
        assert_eq!(start, "Hello W");
        assert_eq!(bufs.remaining(), 4);
    }

    #[test]
    fn one_long_buf_to_bytes() {
        let mut buf = BufList::new();
        buf.push(b"Hello World" as &[_]);
        assert_eq!(buf.copy_to_bytes(5), "Hello");
        assert_eq!(buf.chunk(), b" World");
    }

    #[test]
    #[should_panic(expected = "`len` greater than remaining")]
    fn buf_to_bytes_too_many() {
        hello_world_buf().copy_to_bytes(42);
    }
}
