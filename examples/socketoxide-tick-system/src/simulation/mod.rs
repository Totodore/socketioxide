pub mod runner;

pub enum ThreadEvent {
    Shutdown,
    ChangeTargetTPS(usize),
}
