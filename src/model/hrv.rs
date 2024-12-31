//! HRV (Heart Rate Variability) Model
//!
//! This module defines the data structures and methods for managing HRV data.
//! It provides functionality for storing, retrieving, and analyzing HRV-related statistics,
//! including calculations of RMSSD, SDRR, and Poincaré plot metrics.
//! The module processes raw heart rate data and computes various HRV parameters used
//! in the analysis of heart rate variability.

use super::bluetooth::HeartrateMessage;
use anyhow::{anyhow, Result};
use hrv_algos::analysis::dfa::{DFAnalysis, DetrendStrategy};
use hrv_algos::analysis::nonlinear::calc_poincare_metrics;
use hrv_algos::analysis::time::{calc_rmssd, calc_sdrr};
use hrv_algos::preprocessing::outliers::{MovingQuantileFilter, OutlierClassifier};

use rayon::iter::{
    IndexedParallelIterator, IntoParallelIterator, IntoParallelRefIterator, ParallelIterator,
};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::ops::Range;
use time::Duration;

/// Represents inliers and outliers on the Poincare plot.
pub type PoincarePoints = (Vec<[f64; 2]>, Vec<[f64; 2]>);

/// Manages runtime data related to HRV analysis.
///
/// This structure collects RR intervals, heart rate values, and timestamps.
/// It processes incoming heart rate measurements and computes HRV statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HrvAnalysisData {
    data: MovingQuantileFilter,
    rr_timepoints: Vec<Duration>,
    /// Time series of RMSSD values.
    rmssd_ts: Vec<[f64; 2]>,
    /// Time series of SDRR values.
    sdrr_ts: Vec<[f64; 2]>,
    /// Time series of SD1 values.
    sd1_ts: Vec<[f64; 2]>,
    /// Time series of SD2 values.
    sd2_ts: Vec<[f64; 2]>,
    /// Time series of heart rate values.
    hr_ts: Vec<[f64; 2]>,
    /// Time series of DFA alpha values
    dfa_alpha_ts: Vec<[f64; 2]>,
}

impl Default for HrvAnalysisData {
    fn default() -> Self {
        Self {
            data: MovingQuantileFilter::new(None, None, None),
            rr_timepoints: Vec::new(),
            rmssd_ts: Vec::new(),
            sdrr_ts: Vec::new(),
            sd1_ts: Vec::new(),
            sd2_ts: Vec::new(),
            hr_ts: Vec::new(),
            dfa_alpha_ts: Vec::new(),
        }
    }
}

/// Represents data collected during an HRV (Heart Rate Variability) session.
///
/// This struct holds heart rate values, RR intervals, reception timestamps, and HRV statistics.
/// It provides methods for processing raw acquisition data, filtering outliers, and calculating
/// HRV statistics.
impl HrvAnalysisData {
    /// Creates an `HrvSessionData` instance from acquisition data.
    ///
    /// Processes raw acquisition data, applies optional time-based filtering,
    /// filters outliers from the RR intervals, and calculates HRV statistics.
    ///
    /// # Arguments
    ///
    /// * `data` - A slice of `(Duration, HeartrateMessage)` tuples representing
    ///   time-stamped heart rate measurements.
    /// * `window` - An optional `Duration` specifying the time window for filtering data.
    ///   Only measurements within this window will be included.
    /// * `outlier_filter` - A threshold value used for identifying and removing outliers
    ///   in RR intervals.
    ///
    /// # Returns
    ///
    /// Returns an `Ok(HrvSessionData)` if the processing succeeds, or an `Err` if HRV
    /// statistics calculation fails (e.g., due to insufficient data).
    pub fn from_acquisition(
        data: &[(Duration, HeartrateMessage)],
        window: Option<usize>,
        outlier_filter: f64,
    ) -> Result<Self> {
        let mut new = Self::default();
        if data.is_empty() {
            return Ok(new);
        }
        new.data.set_quantile_scale(outlier_filter)?;
        new.add_measurements(data, window.unwrap_or(usize::MAX))?;

        Ok(new)
    }

    fn calc_time_series<
        'a,
        T: Send + Sync + 'a,
        R: Send + Sync,
        F: Fn(&[T]) -> Result<R> + Send + Sync,
    >(
        start: usize,
        window: usize,
        data: &[T],
        time: &[Duration],
        func: F,
    ) -> Result<(Vec<R>, Vec<Duration>)> {
        if start >= data.len() {
            return Err(anyhow!("start index out of bounds"));
        }
        if data.len() != time.len() {
            return Err(anyhow!("data and time series length mismatch"));
        }
        Ok(time
            .into_par_iter()
            .enumerate()
            .skip(start)
            .filter_map(|(idx, ts)| {
                let rr = &data[idx.saturating_sub(window)..idx + 1];
                if let Ok(res) = func(rr) {
                    Some((res, *ts))
                } else {
                    None
                }
            })
            .unzip())
    }

    pub fn add_measurement(&mut self, hrs_msg: &HeartrateMessage, window: usize) -> Result<()> {
        // add rr point
        self.add_measurements(&[(Duration::default(), *hrs_msg)], window)
    }

    fn get_last_filtered(&self, window: Range<usize>) -> Result<(Vec<f64>, Vec<Duration>)> {
        if window.end > self.data.get_data().len() {
            return Err(anyhow!("window end out of bounds"));
        }
        let data = self.data.get_data();
        let classes = self.data.get_classification();
        Ok(window
            .into_par_iter()
            .filter_map(|idx| {
                if classes[idx].is_outlier() {
                    None
                } else {
                    Some((data[idx], self.rr_timepoints[idx]))
                }
            })
            .unzip())
    }

    fn calc_statistics(&mut self, start: usize, window: usize) -> Result<()> {
        let start_idx = start.saturating_sub(window);
        let start_win = start.saturating_sub(start_idx);
        let (filtered_rr, filtered_ts) =
            self.get_last_filtered(start..self.data.get_data().len())?;
        {
            let (mut new_data, ts) =
                Self::calc_time_series(start_win, window, &filtered_rr, &filtered_ts, |win| {
                    calc_rmssd(win)
                })?;
            self.rmssd_ts.extend(
                new_data
                    .drain(..)
                    .zip(ts)
                    .map(|(data, ts)| [ts.as_seconds_f64(), data]),
            );
        }
        {
            let (mut new_data, ts) =
                Self::calc_time_series(start_win, window, &filtered_rr, &filtered_ts, |win| {
                    calc_sdrr(win)
                })?;
            self.sdrr_ts.extend(
                new_data
                    .drain(..)
                    .zip(ts)
                    .map(|(data, ts)| [ts.as_seconds_f64(), data]),
            );
        }
        {
            let (mut new_data, ts) =
                Self::calc_time_series(start_win, window, &filtered_rr, &filtered_ts, |win| {
                    let dfa = DFAnalysis::udfa(
                        win,
                        &[4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                        DetrendStrategy::Linear,
                    )?;
                    Ok(dfa.alpha)
                })?;
            self.dfa_alpha_ts.extend(
                new_data
                    .drain(..)
                    .zip(ts)
                    .map(|(data, ts)| [ts.as_seconds_f64(), data]),
            );
        }
        {
            let (new_data, ts) =
                Self::calc_time_series(start_win, window, &filtered_rr, &filtered_ts, |win| {
                    let res = calc_poincare_metrics(win)?;
                    Ok((res.sd1, res.sd2))
                })?;
            let (mut new_sd1_ts, mut new_sd2_ts): (Vec<_>, Vec<_>) = new_data.into_iter().unzip();
            self.sd1_ts.extend(
                new_sd1_ts
                    .drain(..)
                    .zip(ts.iter().cloned())
                    .map(|(data, ts)| [ts.as_seconds_f64(), data]),
            );
            self.sd2_ts.extend(
                new_sd2_ts
                    .drain(..)
                    .zip(ts)
                    .map(|(data, ts)| [ts.as_seconds_f64(), data]),
            );
        }
        {
            let (mut new_data, ts) =
                Self::calc_time_series(start_win, window, &filtered_rr, &filtered_ts, |rr| {
                    Ok(60000.0 * rr.len() as f64 / rr.iter().sum::<f64>())
                })?;
            self.hr_ts.extend(
                new_data
                    .drain(..)
                    .zip(ts)
                    .map(|(data, ts)| [ts.as_seconds_f64(), data]),
            );
        }
        Ok(())
    }

    /// Adds a heart rate measurement to the session data.
    ///
    /// Updates the session with RR intervals, heart rate values, and reception timestamps
    /// extracted from the provided `HeartrateMessage`.
    ///
    /// # Arguments
    ///
    /// * `hrs_msg` - The `HeartrateMessage` containing HR and RR interval data.
    /// * `elapsed_time` - The timestamp associated with the message.
    fn add_measurements(
        &mut self,
        hrs_msgs: &[(Duration, HeartrateMessage)],
        window: usize,
    ) -> Result<()> {
        let rr: Vec<_> = hrs_msgs
            .par_iter()
            .map(|(_, hrs_msg)| {
                hrs_msg
                    .get_rr_intervals()
                    .iter()
                    .filter_map(|&rr| if rr > 0 { Some(f64::from(rr)) } else { None })
                    .collect::<Vec<f64>>()
            })
            .flatten()
            .collect();
        let rr_len = rr.len();
        self.data.add_data(&rr)?;
        self.rr_timepoints.extend(rr.iter().scan(
            *self.rr_timepoints.last().unwrap_or(&Duration::default()),
            |acc, &rr| {
                *acc += Duration::milliseconds(rr as i64);
                Some(*acc)
            },
        ));

        if let Err(e) =
            self.calc_statistics(self.data.get_data().len().saturating_sub(rr_len), window)
        {
            log::warn!("error calculating statistics: {}", e);
        }
        Ok(())
    }

    /// Returns a list of Poincaré plot points.
    ///
    /// # Returns
    ///
    /// A tuple containing two lists of `[x, y]` points: the first list contains inlier points,
    /// and the second list contains outlier points.
    pub fn get_poincare(&self, window: Option<usize>) -> Result<PoincarePoints> {
        let data = self.data.get_data();
        let classes = self.data.get_classification();
        if data.len() < 2 {
            return Err(anyhow!("too few rr intervals for poincare points"));
        }
        let start = window.map(|s| data.len().saturating_sub(s)).unwrap_or(0);
        let mut inliers = Vec::with_capacity(window.unwrap_or(data.len()));
        let mut outliers = Vec::with_capacity(window.unwrap_or(data.len()));
        for (rr, classes) in data.windows(2).zip(classes.windows(2)).skip(start) {
            if classes[0].is_outlier() || classes[1].is_outlier() {
                outliers.push([rr[0], rr[1]]);
            } else {
                inliers.push([rr[0], rr[1]]);
            }
        }
        inliers.shrink_to_fit();
        outliers.shrink_to_fit();

        Ok((inliers, outliers))
    }

    /// Checks if there is sufficient data for HRV calculations.
    ///
    /// # Returns
    ///
    /// `true` if there are enough RR intervals to perform HRV analysis; `false` otherwise.
    #[allow(dead_code)]
    pub fn has_sufficient_data(&self) -> bool {
        self.data.get_data().len() >= 4
    }

    pub fn get_rmssd_ts(&self) -> &[[f64; 2]] {
        &self.rmssd_ts
    }

    pub fn get_sdrr_ts(&self) -> &[[f64; 2]] {
        &self.sdrr_ts
    }
    pub fn get_sd1_ts(&self) -> &[[f64; 2]] {
        &self.sd1_ts
    }
    pub fn get_sd2_ts(&self) -> &[[f64; 2]] {
        &self.sd2_ts
    }
    pub fn get_hr_ts(&self) -> &[[f64; 2]] {
        &self.hr_ts
    }
    pub fn get_dfa_alpha_ts(&self) -> &[[f64; 2]] {
        &self.dfa_alpha_ts
    }
    pub fn get_rmssd(&self) -> Option<f64> {
        self.rmssd_ts.last().map(|v| v[1])
    }
    pub fn get_sdrr(&self) -> Option<f64> {
        self.sdrr_ts.last().map(|v| v[1])
    }
    pub fn get_sd1(&self) -> Option<f64> {
        self.sd1_ts.last().map(|v| v[1])
    }
    pub fn get_sd2(&self) -> Option<f64> {
        self.sd2_ts.last().map(|v| v[1])
    }
    pub fn get_hr(&self) -> Option<f64> {
        self.hr_ts.last().map(|v| v[1])
    }
    pub fn get_dfa_alpha(&self) -> Option<f64> {
        self.dfa_alpha_ts.last().map(|v| v[1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hrv_runtime_data_add_measurement() {
        let mut runtime = HrvAnalysisData::default();
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        let data = [
            (Duration::milliseconds(0), hr_msg),
            (Duration::milliseconds(1000), hr_msg),
            (Duration::milliseconds(2000), hr_msg),
            (Duration::milliseconds(3000), hr_msg),
        ];
        runtime.add_measurements(&data[0..1], 50).unwrap();
        assert!(!runtime.has_sufficient_data());
        runtime.add_measurements(&data[1..], 50).unwrap();
        assert!(runtime.has_sufficient_data());
    }

    #[test]
    fn test_hrv_session_data_from_acquisition() {
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        let data = vec![
            (Duration::milliseconds(0), hr_msg),
            (Duration::milliseconds(1000), hr_msg),
            (Duration::milliseconds(2000), hr_msg),
            (Duration::milliseconds(3000), hr_msg),
        ];
        let session_data = HrvAnalysisData::from_acquisition(&data, None, 50.0).unwrap();
        assert!(session_data.has_sufficient_data());
    }
}
