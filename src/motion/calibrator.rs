/// Calibrates a motion baseline from periodic rocking (e.g., Snoo bassinet).
///
/// During calibration, collects frame-to-frame intensity values
/// and computes `mean + 2 * std_dev` as the baseline. This captures
/// 95% of the rocking motion, so only movement above this level
/// triggers alerts.
pub struct SnooCalibrator {
    intensities: Vec<f64>,
}

impl SnooCalibrator {
    pub fn new() -> Self {
        Self {
            intensities: Vec::new(),
        }
    }

    /// Add an intensity sample.
    pub fn add_sample(&mut self, intensity: f64) {
        self.intensities.push(intensity);
    }

    /// Number of samples collected.
    pub fn sample_count(&self) -> usize {
        self.intensities.len()
    }

    /// Compute the calibrated baseline: mean + 2 * std_dev.
    /// Returns None if no samples were collected.
    pub fn compute_baseline(&self) -> Option<f64> {
        if self.intensities.is_empty() {
            return None;
        }

        let n = self.intensities.len() as f64;
        let mean = self.intensities.iter().sum::<f64>() / n;
        let variance = self.intensities.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        let std_dev = variance.sqrt();

        Some(mean + 2.0 * std_dev)
    }

    /// Compute just the mean (useful for logging).
    pub fn mean(&self) -> f64 {
        if self.intensities.is_empty() {
            return 0.0;
        }
        self.intensities.iter().sum::<f64>() / self.intensities.len() as f64
    }

    /// Compute just the std_dev (useful for logging).
    pub fn std_dev(&self) -> f64 {
        if self.intensities.is_empty() {
            return 0.0;
        }
        let mean = self.mean();
        let n = self.intensities.len() as f64;
        let variance = self.intensities.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
        variance.sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_calibrator_returns_none() {
        let cal = SnooCalibrator::new();
        assert!(cal.compute_baseline().is_none());
    }

    #[test]
    fn constant_signal_baseline_equals_value() {
        let mut cal = SnooCalibrator::new();
        for _ in 0..100 {
            cal.add_sample(0.05);
        }
        let baseline = cal.compute_baseline().unwrap();
        // std_dev ≈ 0, so baseline ≈ mean = 0.05
        assert!((baseline - 0.05).abs() < 1e-10);
    }

    #[test]
    fn sinusoidal_rocking_baseline() {
        let mut cal = SnooCalibrator::new();
        // Simulate sinusoidal rocking motion
        for i in 0..300 {
            let t = i as f64 / 300.0 * std::f64::consts::TAU * 5.0; // 5 cycles
            let intensity = 0.05 + 0.03 * t.sin().abs(); // baseline 0.05, amplitude 0.03
            cal.add_sample(intensity);
        }
        let baseline = cal.compute_baseline().unwrap();
        // Should be above mean but capture rocking
        assert!(baseline > 0.05);
        assert!(baseline < 0.15); // Shouldn't be absurdly high
    }

    #[test]
    fn baseline_captures_95_percent() {
        let mut cal = SnooCalibrator::new();
        // Normal distribution-like samples
        let samples = vec![
            0.04, 0.05, 0.06, 0.05, 0.04, 0.07, 0.03, 0.05, 0.06, 0.05,
            0.04, 0.05, 0.06, 0.05, 0.04, 0.07, 0.03, 0.05, 0.06, 0.05,
        ];
        for s in &samples {
            cal.add_sample(*s);
        }
        let baseline = cal.compute_baseline().unwrap();
        // Count how many are below baseline
        let below = samples.iter().filter(|&&s| s <= baseline).count();
        let pct = below as f64 / samples.len() as f64;
        assert!(pct >= 0.90); // At least 90% should be below mean+2σ
    }
}
