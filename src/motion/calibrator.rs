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

    /// Per-cell (mean, std_dev) for logging and detector initialization.
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
    fn empty_calibrator_returns_zero_stats() {
        let cal = GridCalibrator::new(4);
        assert_eq!(cal.sample_count(), 0);
        let stats = cal.cell_stats();
        assert!(stats.iter().all(|&(m, s)| m == 0.0 && s == 0.0));
    }

    #[test]
    fn constant_cells() {
        let mut cal = GridCalibrator::new(3);
        for _ in 0..50 {
            cal.add_samples(&[0.1, 0.2, 0.3]);
        }
        let stats = cal.cell_stats();
        assert!((stats[0].0 - 0.1).abs() < 1e-10);
        assert!((stats[1].0 - 0.2).abs() < 1e-10);
        assert!((stats[2].0 - 0.3).abs() < 1e-10);
        // std_dev should be ~0 for constant input
        for &(_, s) in &stats {
            assert!(s < 1e-10);
        }
    }

    #[test]
    fn varying_cells() {
        let mut cal = GridCalibrator::new(2);
        for i in 0..100 {
            let t = i as f64 / 100.0 * std::f64::consts::TAU;
            cal.add_samples(&[0.05, 0.05 + 0.03 * t.sin().abs()]);
        }
        let stats = cal.cell_stats();
        // Cell 0: constant, no variance
        assert!((stats[0].0 - 0.05).abs() < 1e-10);
        assert!(stats[0].1 < 1e-10);
        // Cell 1: has variance from sine
        assert!(stats[1].1 > stats[0].1);
    }
}