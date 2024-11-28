//! HRV Model
//!
//! This module defines the data structures and methods for managing HRV (Heart Rate Variability) data.
//! It provides functionality for storing, retrieving, and analyzing HRV-related statistics.

use super::bluetooth::HeartrateMessage;
use crate::math::hrv::{calc_poincare_metrics, calc_rmssd, calc_sdrr};
use log::info;
use nalgebra::DVector;
use std::fmt::Debug;
use time::Duration;

/// Stores heart rate variability (HRV) statistics results.
#[derive(Default, Clone, Debug)]
/// `HrvStatistics` structure.
///
/// Represents data related to HRV analysis, including statistics and session details.
pub struct HrvStatistics {
    /// Root Mean Square of Successive Differences.
    pub rmssd: f64,
    /// Standard Deviation of RR intervals.
    #[allow(dead_code)]
    pub sdrr: f64,
    /// Poincare SD1 (short-term HRV).
    pub sd1: f64,
    /// Eigenvector for SD1.
    #[allow(dead_code)]
    pub sd1_eigenvec: [f64; 2],
    /// Poincare SD2 (long-term HRV).
    pub sd2: f64,
    /// Eigenvector for SD2.
    #[allow(dead_code)]
    pub sd2_eigenvec: [f64; 2],
    /// Ratio of SD1 to SD2.
    #[allow(dead_code)]
    pub sd1_sd2_ratio: f64,
    /// Average heart rate.
    pub avg_hr: f64,
    /// Time window used for the calculation.
    #[allow(dead_code)]
    pub time_window: Duration,
}

/// `HrvSessionData` structure.
///
/// Manages runtime data related to HRV analysis, including RR intervals, heart rate values,
/// and the calculated HRV statistics.
#[derive(Default, Debug)]
pub struct HrvSessionData {
    /// RR intervals in milliseconds.
    pub rr_intervals: Vec<f64>,
    /// Cumulative time for each RR interval.
    pub rr_time: Vec<Duration>,
    /// Heart rate values.
    pub hr_values: Vec<f64>,
    /// Reception timestamps.
    pub rx_time: Vec<Duration>,
    /// Optional time window for statistics.
    pub stats_window: Option<Duration>,
    /// Calculated HRV statistics.
    pub hrv_stats: Option<HrvStatistics>,
}

impl HrvSessionData {
    /// Adds an RR interval measurement to the runtime data.
    fn add_rr_measurement(&mut self, rr_measurement: u16) {
        let rr_ms = rr_measurement as f64;
        let cumulative_time = if let Some(last) = self.rr_time.last() {
            *last + Duration::milliseconds(rr_measurement as i64)
        } else {
            Duration::milliseconds(rr_measurement as i64)
        };

        self.rr_intervals.push(rr_ms);
        self.rr_time.push(cumulative_time);
    }

    /// Returns the current statistics window, if any.
    #[allow(dead_code)]
    pub fn get_stats_window(&self) -> &Option<Duration> {
        &self.stats_window
    }

    /// Sets the statistics window for HRV analysis.
    pub fn set_stats_window(&mut self, window: Option<Duration>) {
        info!("Setting stats window: {:?}", window);
        self.stats_window = window;
    }

    /// Returns the calculated HRV statistics, if available.
    #[allow(dead_code)]
    pub fn get_hrv_stats(&self) -> &Option<HrvStatistics> {
        &self.hrv_stats
    }

    /// Adds an HR service message to the runtime data.
    pub fn add_measurement(&mut self, hrs_msg: &HeartrateMessage, elapsed_time: &Duration) {
        info!("Adding measurement to runtime: {}", hrs_msg);
        for &rr_interval in hrs_msg.get_rr_intervals() {
            self.add_rr_measurement(rr_interval);
        }
        self.hr_values.push(hrs_msg.get_hr());
        self.rx_time.push(*elapsed_time);
    }

    /// Updates HRV statistics based on the current data.
    pub fn update_stats(&mut self) {
        if self.has_sufficient_data() {
            info!("Updating HRV statistics.");
            self.hrv_stats = Some(HrvStatistics::new(self, self.stats_window));
        } else {
            info!("Not enough data to update HRV statistics.");
        }
    }

    /// Returns a list of Poincare plot points.
    pub fn get_poincare(&self) -> Vec<[f64; 2]> {
        self.rr_intervals
            .windows(2)
            .map(|win| [win[0], win[1]])
            .collect()
    }

    /// Checks if there is sufficient data for HRV calculations.
    pub fn has_sufficient_data(&self) -> bool {
        self.rr_intervals.len() >= 4
    }
}

impl HrvStatistics {
    /// Constructs a new `HrvStatistics` from runtime data and an optional time window.
    pub fn new(data: &HrvSessionData, window: Option<Duration>) -> Self {
        if data.rr_intervals.len() < 4 {
            info!("Not enough RR intervals for HRV stats calculation.");
            return Self::default();
        }

        let (rr_intervals, elapsed_duration) = if let Some(window) = window {
            let start_time = *data.rr_time.last().unwrap() - window;
            let indices: Vec<usize> = data
                .rr_time
                .iter()
                .enumerate()
                .filter(|(_, &time)| time >= start_time)
                .map(|(i, _)| i)
                .collect();
            (
                data.rr_intervals[indices[0]..].to_vec(),
                *data.rr_time.last().unwrap() - start_time,
            )
        } else {
            (data.rr_intervals.clone(), *data.rr_time.last().unwrap())
        };

        let avg_hr = if data.hr_values.is_empty() {
            0.0
        } else {
            DVector::from_row_slice(&data.hr_values).mean()
        };

        let poincare = calc_poincare_metrics(&rr_intervals);

        info!("Calculating HRV stats.");
        HrvStatistics {
            rmssd: calc_rmssd(&rr_intervals),
            sdrr: calc_sdrr(&rr_intervals),
            sd1: poincare.sd1,
            sd1_eigenvec: poincare.sd1_eigenvector,
            sd2: poincare.sd2,
            sd2_eigenvec: poincare.sd2_eigenvector,
            sd1_sd2_ratio: poincare.sd1 / poincare.sd2,
            avg_hr,
            time_window: window.unwrap_or(elapsed_duration),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hrv_runtime_data_add_measurement() {
        let mut runtime = HrvSessionData::default();
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        runtime.add_measurement(&hr_msg, &Duration::milliseconds(500));
        assert!(!runtime.has_sufficient_data());
        runtime.add_measurement(&hr_msg, &Duration::milliseconds(500));
        runtime.add_measurement(&hr_msg, &Duration::milliseconds(500));
        runtime.add_measurement(&hr_msg, &Duration::milliseconds(500));
        assert!(runtime.has_sufficient_data());
    }
}
