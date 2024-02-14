#[cfg(feature = "custom-report")]
use charts_rs::LineChart;
use hyper::server::conn::http1;
use hyper_util::rt::TokioIo;
use socketioxide::{
    extract::{AckSender, SocketRef},
    SocketIo,
};
use std::net::SocketAddr;
#[cfg(feature = "custom-report")]
use std::sync::{atomic::AtomicUsize, Mutex};
#[cfg(feature = "custom-report")]
use std::time::Instant;
use tokio::net::TcpListener;
#[cfg(feature = "custom-report")]
use tracing::Level;
#[cfg(feature = "custom-report")]
use tracing_subscriber::FmtSubscriber;

use std::alloc::{GlobalAlloc, Layout, System};

struct MyAllocator;

#[cfg(feature = "custom-report")]
static CURRENT: AtomicUsize = AtomicUsize::new(0);
#[cfg(feature = "custom-report")]
static MEMORY: Mutex<Vec<(Instant, usize, usize)>> = Mutex::new(Vec::new());
#[cfg(feature = "custom-report")]
static SOCKETS_CNT: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for MyAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        #[cfg(feature = "custom-report")]
        CURRENT.fetch_add(layout.size(), std::sync::atomic::Ordering::Relaxed);
        System.alloc(layout)
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        #[cfg(feature = "custom-report")]
        CURRENT.fetch_sub(layout.size(), std::sync::atomic::Ordering::Relaxed);
        System.dealloc(ptr, layout)
    }
}

#[global_allocator]
static GLOBAL: MyAllocator = MyAllocator;

fn on_connect(socket: SocketRef) {
    #[cfg(feature = "custom-report")]
    SOCKETS_CNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    socket.on("ping", |ack: AckSender| {
        ack.send("pong").ok();
    });
    socket.on_disconnect(|| {
        #[cfg(feature = "custom-report")]
        SOCKETS_CNT.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    });
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (svc, io) = SocketIo::new_svc();
    #[cfg(feature = "custom-report")]
    {
        let subscriber = FmtSubscriber::builder()
            .with_line_number(true)
            .with_max_level(Level::TRACE)
            .finish();
        tracing::subscriber::set_global_default(subscriber)?;
    }

    io.ns("/", on_connect);

    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = TcpListener::bind(addr).await?;

    #[cfg(feature = "custom-report")]
    tokio::task::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
            MEMORY.lock().unwrap().push((
                Instant::now(),
                SOCKETS_CNT.load(std::sync::atomic::Ordering::Relaxed),
                CURRENT.load(std::sync::atomic::Ordering::Relaxed),
            ));
        }
    });

    #[cfg(feature = "custom-report")]
    tokio::task::spawn(async move {
        tokio::signal::ctrl_c().await.unwrap();
        println!("saving charts...");
        let points = MEMORY.lock().unwrap();
        let mut time: Vec<String> = Vec::with_capacity(points.len());
        let mut sockets: Vec<String> = Vec::with_capacity(points.len());
        let mut memory: Vec<f32> = Vec::with_capacity(points.len());
        for (t, s, m) in points.iter() {
            time.push(t.elapsed().as_secs_f64().to_string());
            sockets.push(s.to_string());
            memory.push(*m as f32);
        }
        let line_chart = LineChart::new(vec![("Memory", memory).into()], sockets);
        let svg = line_chart.svg().unwrap();
        std::fs::write("memory_usage.svg", svg).unwrap();
        std::process::exit(0);
    });

    loop {
        let (stream, _) = listener.accept().await?;

        let io = TokioIo::new(stream);
        let svc = svc.clone();

        tokio::task::spawn(async move {
            http1::Builder::new()
                .serve_connection(io, svc)
                .with_upgrades()
                .await
                .ok()
        });
    }
}
