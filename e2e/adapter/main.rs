use std::env::args;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::thread::JoinHandle;
use std::time::Duration;
use std::{fs, thread};

const BINS: [&str; 12] = [
    "fred-e2e",
    "fred-e2e-msgpack",
    "redis-e2e",
    "redis-e2e-msgpack",
    "redis-cluster-e2e",
    "redis-cluster-e2e-msgpack",
    "fred-cluster-e2e",
    "fred-cluster-e2e-msgpack",
    "mongodb-ttl-e2e",
    "mongodb-ttl-e2e-msgpack",
    "mongodb-capped-e2e",
    "mongodb-capped-e2e-msgpack",
];
const EXEC_SUFFIX: &str = if cfg!(windows) { ".exe" } else { "" };
const LOG_DIR: &str = "e2e/adapter/logs";
static PORT: AtomicUsize = AtomicUsize::new(3000);
static CHILDREN: Mutex<Vec<Child>> = Mutex::new(Vec::new());

fn main() {
    // take_hook() returns the default hook in case when a custom one is not set
    let orig_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // invoke the default handler and exit the process
        orig_hook(panic_info);
        println!("closing children");
        for mut child in CHILDREN.lock().unwrap().drain(..) {
            child.kill().unwrap();
        }
        std::process::exit(1);
    }));

    let filter = args().skip(1).next().unwrap_or("".to_string());
    println!("filter: {}", filter);

    if fs::exists(LOG_DIR).unwrap() {
        fs::remove_dir_all(LOG_DIR).unwrap();
    }
    fs::create_dir_all(LOG_DIR).unwrap();

    // run everything
    let mut join_set: Vec<JoinHandle<()>> = Vec::new();
    for target in BINS.into_iter().filter(|name| name.contains(&filter)) {
        join_set.push(thread::spawn(move || run(target)));
    }
    for handle in join_set {
        handle.join().unwrap();
    }
    println!("All tests passed!");

    println!("killing children");
    for mut child in CHILDREN.lock().unwrap().drain(..) {
        child.kill().unwrap();
    }
}

fn run(target: &'static str) {
    let parser = if target.ends_with("msgpack") {
        "msgpack"
    } else {
        "json"
    };

    let ports = allocate_ports(3);
    let ports = ports..ports + 3;

    for port in ports.clone() {
        run_server(target, port);
    }

    std::thread::sleep(Duration::from_millis(200));

    let ports = ports
        .into_iter()
        .map(|port| port.to_string())
        .collect::<Vec<String>>()
        .join(",");

    let child = Command::new("node")
        .arg("--experimental-strip-types")
        .arg("--test-reporter=spec")
        .arg("--test")
        .arg("e2e/adapter/client.ts")
        .env("PORTS", &ports)
        .env("PARSER", parser)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to run cargo build");
    let output = child.wait_with_output().unwrap();
    println!(
        "{target} output with status: {}\n {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        panic!(
            "Could not run node client e2e test: {ports}, {parser}, {}",
            output.status
        );
    }
}

fn run_server(target: &'static str, port: usize) {
    println!("Running {} with port {}", target, port);
    let mode = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let path = format!("target/{}/{}{}", mode, target, EXEC_SUFFIX);
    let log = std::fs::File::create(format!("e2e/adapter/logs/{}-{}.log", target, port)).unwrap();
    let child = Command::new(path)
        .env("PORT", port.to_string())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("Failed to run command");
    CHILDREN.lock().unwrap().push(child);
}

fn allocate_ports(cnt: usize) -> usize {
    PORT.fetch_add(cnt, std::sync::atomic::Ordering::SeqCst)
}
