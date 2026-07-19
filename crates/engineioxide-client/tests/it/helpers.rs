use std::{
    convert::Infallible,
    fmt,
    marker::PhantomData,
    panic::Location,
    pin::Pin,
    task::{Context, Poll, ready},
    time::Duration,
};

use futures_core::{Stream, future::BoxFuture};
use futures_util::{FutureExt, StreamExt};
use pin_project_lite::pin_project;
use tokio::{sync::mpsc, time};

/// Default deadline for every await in the tests: an enforcement test must
/// fail fast instead of hanging the whole suite.
pub const DEADLINE: Duration = Duration::from_secs(5);

pub trait FutureTestExt: Future + Sized {
    fn timeout(self) -> TimeoutFut<Self>;
    fn timeout_with(self, duration: Duration) -> TimeoutFut<Self>;
}

pin_project! {
    pub struct TimeoutFut<F: Future> {
        #[pin]
        inner: time::Timeout<F>,
        caller: &'static Location<'static>,
        deadline: Duration,
    }
}

impl<F: Future> TimeoutFut<F> {
    fn new(fut: F, deadline: Duration, caller: &'static Location<'static>) -> Self {
        Self {
            caller,
            deadline,
            inner: time::timeout(deadline, fut),
        }
    }
}
impl<F: Future> Future for TimeoutFut<F> {
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let proj = self.project();
        match ready!(proj.inner.poll(cx)) {
            Ok(v) => Poll::Ready(v),
            Err(_) => panic!("timed out after {:?} from {}", proj.deadline, proj.caller),
        }
    }
}
impl<F: Future> FutureTestExt for F {
    #[track_caller]
    fn timeout(self) -> TimeoutFut<F> {
        TimeoutFut::new(self, DEADLINE, Location::caller())
    }

    #[track_caller]
    fn timeout_with(self, deadline: Duration) -> TimeoutFut<F> {
        TimeoutFut::new(self, deadline, Location::caller())
    }
}
mod via {
    pub enum Stream {}
    pub enum Concrete {}
}
pin_project! {
    pub struct NextFut<F: Future, K> {
        #[pin]
        inner: TimeoutFut<F>,
        _phantom: PhantomData<K>
    }
}
impl<F: Future, K> NextFut<F, K> {
    #[track_caller]
    fn new(fut: F) -> Self {
        Self {
            inner: fut.timeout(),
            _phantom: PhantomData,
        }
    }
}

mod private {
    pub enum NextOk {}
    pub enum NextErr {}
    pub enum NextClose {}
}
impl<F: Future<Output = Option<Result<T, E>>>, T: fmt::Debug, E: fmt::Debug> Future
    for NextFut<F, private::NextOk>
{
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let proj = self.project();
        let caller = proj.inner.caller;
        match ready!(proj.inner.poll(cx)) {
            Some(Ok(v)) => Poll::Ready(v),
            Some(Err(v)) => {
                panic!("expected a value from stream, got an error: {v:?} from {caller}")
            }
            None => panic!("called next_ok on a closed stream from: {caller}",),
        }
    }
}
impl<F: Future<Output = Option<Result<T, E>>>, T: fmt::Debug, E: fmt::Debug> Future
    for NextFut<F, private::NextErr>
{
    type Output = E;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let proj = self.project();
        let caller = proj.inner.caller;
        match ready!(proj.inner.poll(cx)) {
            Some(Err(v)) => Poll::Ready(v),
            Some(Ok(v)) => {
                panic!("expected an error from stream, got a value: {v:?} from {caller}")
            }
            None => panic!("called next_err on a closed stream from: {caller}",),
        }
    }
}
impl<F: Future<Output = Option<Result<T, E>>>, T: fmt::Debug, E: fmt::Debug> Future
    for NextFut<F, private::NextClose>
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let proj = self.project();
        let caller = proj.inner.caller;
        match ready!(proj.inner.poll(cx)) {
            Some(v) => panic!("expected a closed stream, got an item: {v:?} from: {caller}"),
            None => Poll::Ready(()),
        }
    }
}

pub trait ClientTestExt<T, E, V> {
    type Fut<'a>: Future + 'a
    where
        Self: 'a;
    fn next_ok(&mut self) -> NextFut<Self::Fut<'_>, private::NextOk>;
    fn next_err(&mut self) -> NextFut<Self::Fut<'_>, private::NextErr>;
    fn next_close(&mut self) -> NextFut<Self::Fut<'_>, private::NextClose>;

    fn _phantom() -> PhantomData<V> {
        PhantomData
    }
}

impl<S: Stream<Item = Result<T, E>> + Unpin + 'static, T: fmt::Debug, E: fmt::Debug>
    ClientTestExt<T, E, via::Stream> for S
{
    type Fut<'a> = futures_util::stream::Next<'a, S>;

    #[track_caller]
    fn next_ok(&mut self) -> NextFut<Self::Fut<'_>, private::NextOk> {
        NextFut::new(self.next())
    }

    #[track_caller]
    fn next_err(&mut self) -> NextFut<Self::Fut<'_>, private::NextErr> {
        NextFut::new(self.next())
    }

    #[track_caller]
    fn next_close(&mut self) -> NextFut<Self::Fut<'_>, private::NextClose> {
        NextFut::new(self.next())
    }
}
impl<T: fmt::Debug + Send + 'static> ClientTestExt<T, Infallible, via::Concrete>
    for mpsc::UnboundedReceiver<T>
{
    type Fut<'a> = BoxFuture<'a, Option<Result<T, Infallible>>>;

    #[track_caller]
    fn next_ok(&mut self) -> NextFut<Self::Fut<'_>, private::NextOk> {
        NextFut::new(async { self.recv().await.map(Ok) }.boxed())
    }

    #[track_caller]
    fn next_err(&mut self) -> NextFut<Self::Fut<'_>, private::NextErr> {
        NextFut::new(async { self.recv().await.map(Ok) }.boxed())
    }

    #[track_caller]
    fn next_close(&mut self) -> NextFut<Self::Fut<'_>, private::NextClose> {
        NextFut::new(async { self.recv().await.map(Ok) }.boxed())
    }
}
