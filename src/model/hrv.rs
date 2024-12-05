//! HRV Model
//!
//! This module defines the data structures and methods for managing HRV (Heart Rate Variability) data.
//! It provides functionality for storing, retrieving, and analyzing HRV-related statistics.

use super::{ bluetooth::HeartrateMessage};
use crate::math::hrv::{self, calc_poincare_metrics, calc_rmssd, calc_sdrr};
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
}

/// `HrvSessionData` structure.
///
/// Manages runtime data related to HRV analysis, including RR intervals, heart rate values,
/// and the calculated HRV statistics.
#[derive(Default, Debug, Clone)]
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
    pub rmssd_ts: Vec<[f64; 2]>,
    pub sd1_ts: Vec<[f64; 2]>,
    pub sd2_ts: Vec<[f64; 2]>,
    pub hr_ts: Vec<[f64; 2]>,
}


impl HrvSessionData {
    pub fn from_acquisition(
        data: &[(Duration, HeartrateMessage)],
        window: Option<Duration>,
        outlier_filter: f64,
    ) -> Self {
        if data.is_empty() {
            return Self::default();
        }

        let mut new = Self{

            stats_window: window,
            ..Default::default()
        };
        // data.hr_values.reserve(additional);
        let start_time = if let Some(window) = window {
            data.last().unwrap().0 - window
        } else {
            data.first().unwrap().0
        };
        for (ts, msg) in data.iter().filter(|val| val.0 >= start_time) {
            new.add_measurement(msg, ts);
        }
        if new.rr_intervals.len() >= 4 {
            // Outlier filter
            let rrt: Vec<f64> = new.rr_intervals.windows(2).map(|w| w[1] - w[0]).collect();
            let filter_win_size = 5;
            (new.rr_intervals, new.rr_time) = new
                .rr_intervals
                .windows(filter_win_size)
                .zip(new.rr_time.windows(filter_win_size))
                .filter_map(|(v, t)| {
                    let rr_diff = (v[filter_win_size/2] - (v.iter().sum::<f64>()/filter_win_size as f64)).abs() * 0.5;
                    if rr_diff >= outlier_filter {
                        None
                    } else {
                        Some((v[filter_win_size/2], t[filter_win_size/2]))
                    }
                })
                .unzip();
            let hrv_stats = HrvStatistics::new(&new.rr_intervals, &new.hr_values);
            let rr_total:Vec<f64> = data.iter().map(|m|{m.1.get_rr_intervals()}).flatten().map(|f|{*f as f64}).collect();
            let ahr = hrv_stats.avg_hr;
            let win = (ahr * window.unwrap_or(Duration::seconds(30)).as_seconds_f64() / 60.0).floor() as  usize;
            let mut et = 0.0;
            for (start_rr, rr) in rr_total.iter().zip(rr_total.windows(win.max(1)).map(|slc|{
                slc.windows(filter_win_size).filter_map(|v|{
                    let rr_diff = (v[filter_win_size/2] - (v.iter().sum::<f64>()/filter_win_size as f64)).abs() * 0.5;
                    if rr_diff >= outlier_filter {
                        None
                    } else {
                        Some(v[filter_win_size/2])
                    }
                })
            })){
                let rr: Vec<f64> = rr.collect();
                let hr = 60000.0  * rr.len() as f64 / rr.iter().sum::<f64>();
                let stats = HrvStatistics::new(&rr, Default::default());
                et += start_rr*1e-3;
                new.rmssd_ts.push([et, stats.rmssd]);
                new.sd1_ts.push([et, stats.sd1]);
                new.sd2_ts.push([et, stats.sd2]);
                new.hr_ts.push([et, hr]);
            }
            new.hrv_stats = Some(hrv_stats);

        }
        new
    }

    /// Returns the current statistics window, if any.
    #[allow(dead_code)]
    pub fn get_stats_window(&self) -> &Option<Duration> {
        &self.stats_window
    }

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

    /// Returns the calculated HRV statistics, if available.
    #[allow(dead_code)]
    pub fn get_hrv_stats(&self) -> &Option<HrvStatistics> {
        &self.hrv_stats
    }

    /// Adds an HR service message to the runtime data.
    fn add_measurement(&mut self, hrs_msg: &HeartrateMessage, elapsed_time: &Duration) {
        for &rr_interval in hrs_msg.get_rr_intervals() {
            self.add_rr_measurement(rr_interval);
        }
        self.hr_values.push(hrs_msg.get_hr());
        self.rx_time.push(*elapsed_time);
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
    fn new(rr_intervals:&[f64], hr_values: &[f64]) -> Self {
        if rr_intervals.len() < 4 {
            info!("Not enough RR intervals for HRV stats calculation.");
            return Self::default();
        }

        let avg_hr = if hr_values.is_empty() {
            0.0
        } else {
            DVector::from_row_slice(&hr_values).mean()
        };

        let poincare = calc_poincare_metrics(rr_intervals);

        HrvStatistics {
            rmssd: calc_rmssd(&rr_intervals),
            sdrr: calc_sdrr(&rr_intervals),
            sd1: poincare.sd1,
            sd1_eigenvec: poincare.sd1_eigenvector,
            sd2: poincare.sd2,
            sd2_eigenvec: poincare.sd2_eigenvector,
            sd1_sd2_ratio: poincare.sd1 / poincare.sd2,
            avg_hr,
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
