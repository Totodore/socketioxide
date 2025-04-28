use std::env::args;
use std::fs;
use std::process::{Child, Command};
use std::time::Duration;

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

fn main() {
    let filter = args().skip(1).next().unwrap_or("".to_string());
    println!("filter: {}", filter);

    if fs::exists(LOG_DIR).unwrap() {
        fs::remove_dir_all(LOG_DIR).unwrap();
    }
    fs::create_dir_all(LOG_DIR).unwrap();

    // run everything
    for target in BINS.into_iter().filter(|name| name.contains(&filter)) {
        run(target);
    }
    println!("All tests passed!");
}

fn run(target: &'static str) {
    let parser = if target.ends_with("msgpack") {
        "msgpack"
    } else {
        "json"
    };

    let mut children: Vec<Child> = Vec::with_capacity(3);
    for port in 3000..=3002 {
        children.push(run_server(target, port));
    }

    std::thread::sleep(Duration::from_millis(200));

    let child = Command::new("node")
        .arg("--experimental-strip-types")
        .arg("--test-reporter=spec")
        .arg("--test")
        .arg("e2e/adapter/client.ts")
        .env("PORTS", "3000,3001,3002")
        .env("PARSER", parser)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("Failed to run cargo build");
    let output = child.wait_with_output().unwrap();
    children.iter_mut().for_each(|c| {
        c.kill().ok();
    });
    println!(
        "{target} output with status: {}\n {}\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if !output.status.success() {
        panic!(
            "Could not run node client e2e test: {parser}, {}",
            output.status
        );
    }
}

fn run_server(target: &'static str, port: usize) -> Child {
    println!("Running {} with port {}", target, port);
    let mode = if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    };
    let path = format!("target/{}/{}{}", mode, target, EXEC_SUFFIX);
    let log = std::fs::File::create(format!("e2e/adapter/logs/{}-{}.log", target, port)).unwrap();
    Command::new(path)
        .env("PORT", port.to_string())
        .stdout(log.try_clone().unwrap())
        .stderr(log)
        .spawn()
        .expect("Failed to run command")
}
