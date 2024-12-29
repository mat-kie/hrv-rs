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
use hrv_algos::preprocessing::noise::ApplyDithering;
use hrv_algos::preprocessing::outliers::{classify_rr_values, OutlierType};

use rayon::iter::{
    IndexedParallelIterator, IntoParallelIterator, IntoParallelRefIterator, ParallelIterator,
};
use serde::{Deserialize, Serialize};
use std::fmt::Debug;
use std::mem::swap;
use time::Duration;

/// Represents inliers and outliers on the Poincare plot.
pub type PoincarePoints = (Vec<[f64; 2]>, Vec<[f64; 2]>);

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct RuntimeRecordingData {
    /// RR intervals in milliseconds.
    rr_intervals: Vec<f64>,
    /// RR interval calssification.
    rr_classification: Vec<OutlierType>,
    /// Cumulative time for each RR interval.
    rr_time: Vec<Duration>,
    outlier_filter: f64,
}
impl RuntimeRecordingData {
    pub fn add_rr(&mut self, rr: &[f64]) -> Result<()> {
        {
            let mut new_ts: Vec<_> = rr
                .iter()
                .scan(
                    *self.rr_time.last().unwrap_or(&Duration::default()),
                    |acc, &rr| {
                        *acc += Duration::milliseconds(rr as i64);
                        Some(*acc)
                    },
                )
                .collect();
            self.rr_time.append(&mut new_ts);
            let mut appended: Vec<_> = rr.iter().copied().apply_dithering(None).collect();
            self.rr_intervals.append(&mut appended);
        }
        self.update_classification()?;
        Ok(())
    }

    pub fn update_classification(&mut self) -> Result<()> {
        const ANALYSIS_WINDOW: usize = 91;
        // classification uses a 91 item rolling quantile
        // take 91 last rr, update the last 46 elements classification
        let win_start = self.rr_classification.len().saturating_sub(ANALYSIS_WINDOW);
        let cutoff = self
            .rr_classification
            .len()
            .saturating_sub(ANALYSIS_WINDOW / 2);
        let added_rr = self
            .rr_intervals
            .len()
            .saturating_sub(self.rr_classification.len());
        let data = &self.rr_intervals[win_start..];
        if data.is_empty() {
            return Ok(());
        }
        let new_class = if data.len() == 1 {
            vec![OutlierType::None]
        } else {
            classify_rr_values(data, None, None, Some(self.outlier_filter))?
        };

        let mut added_classes = new_class[new_class.len().saturating_sub(added_rr)..].to_vec();
        self.rr_classification.append(&mut added_classes);
        //  update the last 46 elements classification
        for (a, b) in self.rr_classification.iter_mut().skip(cutoff).zip(
            new_class
                .iter()
                .skip(new_class.len().saturating_sub(ANALYSIS_WINDOW / 2)),
        ) {
            *a = *b;
        }
        Ok(())
    }

    pub fn set_filter(&mut self, filter: f64) -> Result<()> {
        if filter != self.outlier_filter {
            self.outlier_filter = filter;
            let mut old = Vec::default();
            swap(&mut old, &mut self.rr_classification);
            let res = self.update_classification();
            if res.is_err() {
                self.rr_classification = old;
                return res;
            } else {
                return Ok(());
            }
        }
        Ok(())
    }

    pub fn get_rr(&self) -> &[f64] {
        &self.rr_intervals
    }

    fn get_filtered<T: Send + Sync + Clone>(&self, data: &[T], start_offset: usize) -> Vec<T> {
        data.par_iter()
            .zip(&self.rr_classification)
            .skip(start_offset)
            .filter_map(|(rr, class)| {
                if matches!(class, OutlierType::None) {
                    Some(rr.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    pub fn get_filtered_rr(&self, start_offset: usize) -> Vec<f64> {
        self.get_filtered(&self.rr_intervals, start_offset)
    }
    pub fn get_filtered_ts(&self, start_offset: usize) -> Vec<Duration> {
        self.get_filtered(&self.rr_time, start_offset)
    }
    pub fn get_poincare(&self, samples: Option<usize>) -> Result<PoincarePoints> {
        if self.rr_intervals.len() < 2 {
            return Err(anyhow!("too few rr intervals for poincare points"));
        }
        let start = samples
            .map(|s| self.rr_intervals.len().saturating_sub(s))
            .unwrap_or(0);
        let mut inliers = Vec::with_capacity(samples.unwrap_or(self.rr_intervals.len()));
        let mut outliers = Vec::with_capacity(samples.unwrap_or(self.rr_intervals.len()));
        for (rr, classes) in self
            .rr_intervals
            .windows(2)
            .zip(self.rr_classification.windows(2))
            .skip(start)
        {
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
}

/// Manages runtime data related to HRV analysis.
///
/// This structure collects RR intervals, heart rate values, and timestamps.
/// It processes incoming heart rate measurements and computes HRV statistics.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct HrvAnalysisData {
    data: RuntimeRecordingData,
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
        new.data.set_filter(outlier_filter)?;
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
        let new_rr: Vec<f64> = hrs_msg
            .get_rr_intervals()
            .iter()
            .map(|&rr| f64::from(rr))
            .collect();
        let new_rr_len = new_rr.len();
        if new_rr.is_empty() {
            return Ok(());
        }
        self.data.add_rr(&new_rr)?;
        if let Err(e) =
            self.calc_statistics(self.data.get_rr().len().saturating_sub(new_rr_len), window)
        {
            log::warn!("error calculating statistics: {}", e);
        }
        Ok(())
    }

    fn calc_statistics(&mut self, start: usize, window: usize) -> Result<()> {
        let so = start.saturating_sub(window);
        let start_win = start.saturating_sub(so);
        let filtered_rr = self.data.get_filtered_rr(so);
        let filtered_ts = self.data.get_filtered_ts(so);
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
        self.data.add_rr(&rr)?;

        if let Err(e) =
            self.calc_statistics(self.data.get_rr().len().saturating_sub(rr_len), window)
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
        self.data.get_poincare(window)
    }

    /// Checks if there is sufficient data for HRV calculations.
    ///
    /// # Returns
    ///
    /// `true` if there are enough RR intervals to perform HRV analysis; `false` otherwise.
    #[allow(dead_code)]
    pub fn has_sufficient_data(&self) -> bool {
        self.data.get_rr().len() >= 4
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

    #[test]
    fn test_runtime_recording_data_new() {
        let rr_data = vec![800.0, 810.0, 790.0];
        let outlier_filter = 50.0;
        let mut runtime_data = RuntimeRecordingData::default();
        runtime_data.set_filter(outlier_filter).unwrap();
        runtime_data.add_rr(&rr_data).unwrap();
        rr_data
            .iter()
            .zip(runtime_data.rr_intervals.iter())
            .for_each(|(a, b)| {
                assert!((a - b).abs() < 1.0);
            });
        assert_eq!(runtime_data.outlier_filter, outlier_filter);
    }

    #[test]
    fn test_runtime_recording_data_add_rr() {
        let mut runtime_data = RuntimeRecordingData::default();
        let rr_data = vec![800.0, 810.0, 790.0];
        runtime_data.add_rr(&rr_data).unwrap();
        rr_data
            .iter()
            .zip(runtime_data.rr_intervals.iter())
            .for_each(|(a, b)| {
                assert!((a - b).abs() < 1.0);
            });
    }

    #[test]
    fn test_runtime_recording_data_update_classification() {
        let mut runtime_data = RuntimeRecordingData::default();
        runtime_data.set_filter(5.0).unwrap();
        let rr_data = vec![800.0, 810.0, 790.0];
        runtime_data.add_rr(&rr_data).unwrap();
        assert_eq!(runtime_data.rr_classification.len(), rr_data.len());
    }

    #[test]
    fn test_runtime_recording_data_get_filtered_rr() {
        let mut runtime_data = RuntimeRecordingData::default();
        runtime_data.set_filter(5.0).unwrap();
        let rr_data = vec![800.0, 810.0, 790.0];
        runtime_data.add_rr(&rr_data).unwrap();
        let filtered_rr = runtime_data.get_filtered_rr(0);
        assert_eq!(filtered_rr.len(), rr_data.len());
        assert_eq!(filtered_rr, runtime_data.rr_intervals);
    }
    #[test]
    fn test_runtime_recording_data_get_filtered_rr_all_out() {
        let mut runtime_data = RuntimeRecordingData::default();
        let rr_data = vec![800.0, 810.0, 790.0];
        runtime_data.add_rr(&rr_data).unwrap();
        let filtered_rr = runtime_data.get_filtered_rr(0);
        assert!(filtered_rr.is_empty());
    }
    #[test]
    fn test_runtime_recording_data_get_poincare() {
        let mut runtime_data = RuntimeRecordingData::default();
        runtime_data.set_filter(5.0).unwrap();
        let rr_data = vec![800.0, 810.0, 790.0];
        runtime_data.add_rr(&rr_data).unwrap();
        let (inliers, outliers) = runtime_data.get_poincare(None).unwrap();
        assert!(!inliers.is_empty());
        assert!(outliers.is_empty());
    }
}
