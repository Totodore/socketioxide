use std::time::Duration;

const SECOND_UNIT: f64 = 1.0;

pub struct TickSystem {
    pub target_tps: usize,
    pub timestep: Option<Duration>,
    pub delta: Duration,
    pub overstep: Duration,
    pub elapsed: Duration,
}

impl TickSystem {
    pub fn new(target_tps: usize) -> Self {
        Self {
            target_tps,
            timestep: None,
            delta: Duration::ZERO,
            overstep: Duration::ZERO,
            elapsed: Duration::ZERO,
        }
    }

    fn calculate_timestep(&self) -> Duration {
        if self.timestep.is_some() {
            self.timestep.unwrap()
        } else {
            Duration::try_from_secs_f64(SECOND_UNIT / self.target_tps as f64)
                .unwrap_or(Duration::from_secs_f64(SECOND_UNIT / 20.0))
        }
    }

    pub fn advance_by(&mut self, delta: Duration) {
        self.delta = delta;
        self.elapsed += delta;
    }

    pub fn accumulate(&mut self, delta: Duration) {
        self.overstep += delta;
    }

    pub fn expend(&mut self) -> bool {
        let timestep = self.calculate_timestep();
        if let Some(new_value) = self.overstep.checked_sub(timestep) {
            // reduce accumulated and increase elapsed by period
            self.overstep = new_value;
            self.advance_by(timestep);

            true
        } else {
            // no more periods left in accumulated
            false
        }
    }
}
