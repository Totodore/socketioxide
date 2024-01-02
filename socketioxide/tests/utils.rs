#![allow(dead_code)]

#[macro_export]
macro_rules! assert_ok {
    ($e:expr) => {
        assert_ok!($e,)
    };
    ($e:expr,) => {{
        use std::result::Result::*;
        match $e {
            Ok(v) => v,
            Err(e) => panic!("assertion failed: Err({:?})", e),
        }
    }};
    ($e:expr, $($arg:tt)+) => {{
        use std::result::Result::*;
        match $e {
            Ok(v) => v,
            Err(e) => panic!("assertion failed: Err({:?}): {}", e, format_args!($($arg)+)),
        }
    }};
}

#[macro_export]
macro_rules! assert_err {
    ($e:expr) => {
        assert_err!($e,);
    };
    ($e:expr,) => {{
        use std::result::Result::*;
        match $e {
            Ok(v) => panic!("assertion failed: Ok({:?})", v),
            Err(e) => e,
        }
    }};
    ($e:expr, $($arg:tt)+) => {{
        use std::result::Result::*;
        match $e {
            Ok(v) => panic!("assertion failed: Ok({:?}): {}", v, format_args!($($arg)+)),
            Err(e) => e,
        }
    }};
}

#[macro_export]
macro_rules! assert_some {
    ($e:expr) => {
        assert_some!($e,);
    };
    ($e:expr,) => {{
        use std::option::Option::*;
        match $e {
            Some(v) => v,
            None => panic!("assertion failed: None"),
        }
    }};
    ($e:expr, $($arg:tt)+) => {{
        use std::option::Option::*;
        match $e {
            Some(v) => v,
            None => panic!("assertion failed: None: {}", format_args!($($arg)+)),
        }
    }};
}
