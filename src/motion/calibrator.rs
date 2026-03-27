/// Calibrates a motion baseline from periodic rocking (e.g., Snoo bassinet).
///
/// During calibration, collects frame-to-frame intensity values
/// and computes `mean + 2 * std_dev` as the baseline. This captures
/// 95% of the rocking motion, so only movement above this level
/// triggers alerts.
#[cfg(test)]
pub struct SnooCalibrator {
    intensities: Vec<f64>,
}

#[cfg(test)]
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

// ---------------------------------------------------------------------------
// Grid-based calibrator
// ---------------------------------------------------------------------------

/// Per-cell calibrator for grid-based motion detection.
/// Each cell gets its own baseline (mean + 2σ) since rocking affects regions differently.
pub struct GridCalibrator {
    num_cells: usize,
    cell_samples: Vec<Vec<f64>>,
}

impl GridCalibrator {
    pub fn new(num_cells: usize) -> Self {
        Self {
            num_cells,
            cell_samples: vec![Vec::new(); num_cells],
        }
    }

    /// Add one sample per cell from a single frame.
    pub fn add_samples(&mut self, intensities: &[f64]) {
        assert_eq!(intensities.len(), self.num_cells);
        for (i, &v) in intensities.iter().enumerate() {
            self.cell_samples[i].push(v);
        }
    }

    /// Number of sample frames collected.
    pub fn sample_count(&self) -> usize {
        self.cell_samples.first().map_or(0, |v| v.len())
    }

    /// Compute per-cell baselines: mean + 2 * std_dev for each cell.
    #[cfg(test)]
    pub fn compute_baselines(&self) -> Option<Vec<f64>> {
        if self.sample_count() == 0 {
            return None;
        }
        Some(
            self.cell_samples
                .iter()
                .map(|samples| {
                    let n = samples.len() as f64;
                    let mean = samples.iter().sum::<f64>() / n;
                    let variance = samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
                    mean + 2.0 * variance.sqrt()
                })
                .collect(),
        )
    }

    /// Per-cell (mean, std_dev) for logging.
    pub fn cell_stats(&self) -> Vec<(f64, f64)> {
        self.cell_samples
            .iter()
            .map(|samples| {
                if samples.is_empty() {
                    return (0.0, 0.0);
                }
                let n = samples.len() as f64;
                let mean = samples.iter().sum::<f64>() / n;
                let variance = samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
                (mean, variance.sqrt())
            })
            .collect()
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

    // --- GridCalibrator tests ---

    #[test]
    fn grid_calibrator_empty_returns_none() {
        let cal = GridCalibrator::new(4);
        assert!(cal.compute_baselines().is_none());
    }

    #[test]
    fn grid_calibrator_constant_cells() {
        let mut cal = GridCalibrator::new(3);
        for _ in 0..50 {
            cal.add_samples(&[0.1, 0.2, 0.3]);
        }
        let baselines = cal.compute_baselines().unwrap();
        // std_dev ≈ 0, so baselines ≈ means
        assert!((baselines[0] - 0.1).abs() < 1e-10);
        assert!((baselines[1] - 0.2).abs() < 1e-10);
        assert!((baselines[2] - 0.3).abs() < 1e-10);
    }

    #[test]
    fn grid_calibrator_varying_cells() {
        let mut cal = GridCalibrator::new(2);
        // Cell 0: constant. Cell 1: varies.
        for i in 0..100 {
            let t = i as f64 / 100.0 * std::f64::consts::TAU;
            cal.add_samples(&[0.05, 0.05 + 0.03 * t.sin().abs()]);
        }
        let baselines = cal.compute_baselines().unwrap();
        // Cell 0 baseline ≈ 0.05 (no variance)
        assert!((baselines[0] - 0.05).abs() < 1e-10);
        // Cell 1 baseline > cell 0 baseline (has variance from sine)
        assert!(baselines[1] > baselines[0]);
    }
}
