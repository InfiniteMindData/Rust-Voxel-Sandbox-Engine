//! Frame timing, fixed-timestep simulation ticking and FPS accounting.

use std::time::Duration;

/// Drives a fixed-timestep simulation inside a free-running render loop.
///
/// The classic accumulator pattern: wall-clock time is converted into a whole
/// number of fixed update steps, with a clamp so that a long stall (debugger
/// break, background tab) cannot trigger a "spiral of death" of hundreds of
/// catch-up steps.
#[derive(Debug, Clone)]
pub struct FixedTimestep {
    step: Duration,
    accumulator: Duration,
    max_steps_per_frame: u32,
}

impl FixedTimestep {
    /// Creates a fixed timestep of `step` per simulation tick, running at most
    /// `max_steps_per_frame` catch-up steps in a single frame.
    pub fn new(step: Duration, max_steps_per_frame: u32) -> Self {
        assert!(max_steps_per_frame >= 1, "must allow at least one step per frame");
        Self {
            step,
            accumulator: Duration::ZERO,
            max_steps_per_frame,
        }
    }

    /// The fixed step duration.
    pub fn step(&self) -> Duration {
        self.step
    }

    /// Feeds elapsed wall-clock time into the accumulator and returns how many
    /// full simulation steps should run now.
    pub fn tick(&mut self, elapsed: Duration) -> u32 {
        self.accumulator += elapsed;
        let mut steps = 0u32;
        while self.accumulator >= self.step && steps < self.max_steps_per_frame {
            self.accumulator -= self.step;
            steps += 1;
        }
        if steps == self.max_steps_per_frame {
            // We are falling behind: drop the backlog instead of accumulating
            // an unbounded debt. Simulation time effectively slows down, which
            // is the correct behavior for an interactive game.
            self.accumulator = Duration::ZERO;
        }
        steps
    }

    /// Fraction of a step completed (0.0..1.0). Useful for interpolation when
    /// rendering between simulation states. (Consumed by the player controller
    /// milestone for smooth entity interpolation.)
    #[allow(dead_code)]
    pub fn alpha(&self) -> f32 {
        self.accumulator.as_secs_f32() / self.step.as_secs_f32()
    }
}

/// Rolling FPS / frame-time statistics using an exponential moving average.
#[derive(Debug, Clone)]
pub struct FpsCounter {
    ema: f32,
    /// Half-life of the EMA in frames.
    smoothing: f32,
}

impl FpsCounter {
    /// Creates a counter; `smoothing` is the approximate number of frames
    /// after which a change in frame time has decayed to ~50%.
    pub fn new(smoothing: f32) -> Self {
        assert!(smoothing > 0.0);
        Self {
            ema: 1.0 / 60.0,
            smoothing,
        }
    }

    /// Records one frame with the given duration.
    pub fn record(&mut self, frame_time: Duration) {
        let a = 1.0 - 0.5f32.powf(1.0 / self.smoothing);
        let dt = frame_time.as_secs_f32().clamp(0.0, 10.0);
        self.ema += a * (dt - self.ema);
    }

    /// Smoothed frames-per-second.
    pub fn fps(&self) -> f32 {
        if self.ema <= f32::EPSILON {
            0.0
        } else {
            1.0 / self.ema
        }
    }

    /// Smoothed frame time in milliseconds.
    pub fn frame_ms(&self) -> f32 {
        self.ema * 1000.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestep_emits_one_step_per_interval() {
        let mut ts = FixedTimestep::new(Duration::from_millis(10), 5);
        // 35 ms elapsed -> 3 full steps, 5 ms left in the accumulator.
        assert_eq!(ts.tick(Duration::from_millis(35)), 3);
        assert!((ts.alpha() - 0.5).abs() < 1e-5);
        // Another 10 ms -> exactly one more step.
        assert_eq!(ts.tick(Duration::from_millis(10)), 1);
        assert_eq!(ts.tick(Duration::from_millis(0)), 0);
    }

    #[test]
    fn timestep_clamps_catch_up_debt() {
        let mut ts = FixedTimestep::new(Duration::from_millis(10), 2);
        // A 1 second stall must not queue 100 steps.
        assert_eq!(ts.tick(Duration::from_secs(1)), 2);
        assert_eq!(ts.accumulator, Duration::ZERO, "backlog must be dropped");
    }

    #[test]
    fn fps_counter_converges_upwards() {
        let mut c = FpsCounter::new(8.0);
        for _ in 0..500 {
            c.record(Duration::from_secs_f32(1.0 / 120.0));
        }
        assert!(c.fps() > 100.0, "fps should approach 120, got {}", c.fps());
    }
}
