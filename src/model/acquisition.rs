//! Acquisition Model
//!
//! This module defines the model for managing acquisition data in the HRV analysis tool.
//! It provides structures and traits for handling real-time and stored data related to HRV.

use super::bluetooth::HeartrateMessage;
use crate::model::hrv::{HrvSessionData, HrvStatistics};
use anyhow::Result;
use log::{trace, warn};
use mockall::automock;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::Debug;
use time::{Duration, OffsetDateTime};

/// `AcquisitionModelApi` trait.
///
/// Defines the interface for managing acquisition-related data, including runtime measurements,
/// HRV statistics, and stored acquisitions.
#[automock]
pub trait AcquisitionModelApi: Debug + Send + Sync {
    /// Retrieves the start time of the current acquisition.
    ///
    /// # Returns
    /// An `OffsetDateTime` indicating the start time.
    fn get_start_time(&self) -> OffsetDateTime;

    /// Retrieves the last heart rate message received.
    ///
    /// # Returns
    /// An optional `HeartrateMessage` representing the most recent measurement.
    fn get_last_msg(&self) -> Option<HeartrateMessage>;

    /// Retrieves the current HRV statistics.
    ///
    /// # Returns
    /// A reference to an optional `HrvStatistics` containing computed HRV data.
    fn get_hrv_stats(&self) -> &Option<HrvStatistics>;

    /// Retrieves the configured statistics window.
    ///
    /// # Returns
    /// A reference to an optional `Duration` representing the analysis window size.
    fn get_stats_window(&self) -> &Option<Duration>;

    /// Getter for the filter parameter value (fraction of std. dev).
    ///
    /// # Returns
    /// The parameter value for the outlier filter.
    fn get_outlier_filter_value(&self) -> f64;

    /// Setter for the filter parameter value (fraction of std. dev).
    ///
    /// # Arguments
    /// - `value`: The new filter value.
    ///
    /// # Returns
    /// A result indicating success or failure.
    fn set_outlier_filter_value(&mut self, value: f64) -> Result<()>;

    /// Retrieves the points for the Poincare plot.
    ///
    /// # Returns
    /// A vector of `[f64; 2]` pairs representing the Poincare points.
    fn get_poincare_points(&self) -> Vec<[f64; 2]>;

    /// Adds a new heart rate measurement to the current acquisition.
    ///
    /// # Arguments
    /// - `msg`: The `HeartrateMessage` containing the measurement data.
    ///
    /// # Returns
    /// A result indicating success or failure.
    fn add_measurement(&mut self, msg: &HeartrateMessage) -> Result<()>;

    /// Sets the statistics analysis window.
    ///
    /// # Arguments
    /// - `window`: A `Duration` representing the new analysis window size.
    ///
    /// # Returns
    /// A result indicating success or failure.
    fn set_stats_window(&mut self, window: &Duration) -> Result<()>;

    /// Retrieves the session data.
    ///
    /// # Returns
    /// A reference to the `HrvSessionData`.
    fn get_session_data(&self) -> &HrvSessionData;

    /// Retrieves all heart rate messages with their elapsed time.
    ///
    /// # Returns
    /// A reference to a slice of tuples containing `Duration` and `HeartrateMessage`.
    #[allow(dead_code)]
    fn get_messages(&self) -> &[(Duration, HeartrateMessage)];

    /// Retrieves the elapsed time since the start of the acquisition.
    ///
    /// # Returns
    /// A `Duration` representing the elapsed time.
    fn get_elapsed_time(&self) -> Duration;
}

/// Represents the acquisition model, managing HRV-related data and operations.
#[derive(Serialize, Debug, Clone)]
pub struct AcquisitionModel {
    /// The start time of the acquisition.
    start_time: OffsetDateTime,
    /// Collected measurements with their elapsed time.
    measurements: Vec<(Duration, HeartrateMessage)>,
    /// Window duration for statistical calculations.
    window: Option<Duration>,
    /// Outlier filter threshold.
    outlier_filter: f64,
    /// Processed session data.
    #[serde(skip)]
    sessiondata: HrvSessionData,
}

impl Default for AcquisitionModel {
    fn default() -> Self {
        Self {
            start_time: OffsetDateTime::now_utc(),
            measurements: Vec::new(),
            window: None,
            outlier_filter: 1.0,
            sessiondata: Default::default(),
        }
    }
}

impl<'de> Deserialize<'de> for AcquisitionModel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct AcquisitionModelHelper {
            start_time: OffsetDateTime,
            measurements: Vec<(Duration, HeartrateMessage)>,
            window: Option<Duration>,
            outlier_filter: f64,
        }
        // Deserialize all fields except `sessiondata`
        let helper = AcquisitionModelHelper::deserialize(deserializer)?;

        // Reconstruct `sessiondata` from the `measurements`
        let sessiondata = HrvSessionData::from_acquisition(
            &helper.measurements,
            helper.window,
            helper.outlier_filter,
        )
        .map_err(serde::de::Error::custom)?;

        Ok(AcquisitionModel {
            start_time: helper.start_time,
            measurements: helper.measurements,
            window: helper.window,
            outlier_filter: helper.outlier_filter,
            sessiondata,
        })
    }
}

impl AcquisitionModel {
    /// Updates the session data based on the current measurements.
    ///
    /// # Returns
    /// A result indicating success or failure.
    fn update(&mut self) -> Result<()> {
        match HrvSessionData::from_acquisition(&self.measurements, self.window, self.outlier_filter)
        {
            Ok(data) => self.sessiondata = data,
            Err(e) => {
                warn!("could not calculate session data: {}", e);
            }
        }
        Ok(())
    }
}

impl AcquisitionModelApi for AcquisitionModel {
    fn get_elapsed_time(&self) -> Duration {
        if self.measurements.is_empty() {
            Duration::default()
        } else {
            let (ts, _) = self.measurements.last().unwrap();
            *ts
        }
    }

    fn get_messages(&self) -> &[(Duration, HeartrateMessage)] {
        &self.measurements
    }

    fn get_session_data(&self) -> &HrvSessionData {
        &self.sessiondata
    }

    fn get_hrv_stats(&self) -> &Option<HrvStatistics> {
        &self.sessiondata.hrv_stats
    }

    fn get_poincare_points(&self) -> Vec<[f64; 2]> {
        self.sessiondata.get_poincare()
    }

    fn get_start_time(&self) -> OffsetDateTime {
        self.start_time
    }

    fn get_last_msg(&self) -> Option<HeartrateMessage> {
        self.measurements.last().map(|entry| entry.1)
    }

    fn get_stats_window(&self) -> &Option<Duration> {
        &self.window
    }

    fn add_measurement(&mut self, msg: &HeartrateMessage) -> Result<()> {
        trace!("add HR measurement\n{}", msg);
        let elapsed = OffsetDateTime::now_utc() - self.start_time;
        self.measurements.push((elapsed, *msg));
        self.update()
    }

    fn set_stats_window(&mut self, window: &Duration) -> Result<()> {
        self.window = Some(*window);
        self.update()
    }

    fn get_outlier_filter_value(&self) -> f64 {
        self.outlier_filter
    }

    fn set_outlier_filter_value(&mut self, value: f64) -> Result<()> {
        if value >= 0.0 {
            self.outlier_filter = value;
        }
        self.update()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::bluetooth::HeartrateMessage;
    use time::macros::datetime;

    fn create_test_hr_msg(bpm: u8) -> HeartrateMessage {
        let data = [0b00010000, bpm, 0, 4, 1, 4];
        HeartrateMessage::new(&data)
    }
    fn create_test_model() -> AcquisitionModel {
        AcquisitionModel {
            start_time: datetime!(2023-01-01 00:00:00 UTC),
            measurements: vec![
                (Duration::seconds(1), create_test_hr_msg(60)),
                (Duration::seconds(2), create_test_hr_msg(62)),
            ],
            window: Some(Duration::seconds(60)),
            outlier_filter: 1.0,
            sessiondata: HrvSessionData::default(),
        }
    }

    #[test]
    fn test_get_start_time() {
        let model = create_test_model();
        assert_eq!(model.get_start_time(), datetime!(2023-01-01 00:00:00 UTC));
    }

    #[test]
    fn test_get_last_msg() {
        let model = create_test_model();
        assert_eq!(model.get_last_msg(), Some(create_test_hr_msg(62)));
    }

    #[test]
    fn test_get_hrv_stats() {
        let model = create_test_model();
        assert!(model.get_hrv_stats().is_none());
    }

    #[test]
    fn test_get_stats_window() {
        let model = create_test_model();
        assert_eq!(model.get_stats_window(), &Some(Duration::seconds(60)));
    }

    #[test]
    fn test_get_outlier_filter_value() {
        let model = create_test_model();
        assert_eq!(model.get_outlier_filter_value(), 1.0);
    }

    #[test]
    fn test_set_outlier_filter_value() {
        let mut model = create_test_model();

        model.set_outlier_filter_value(2.0).unwrap();
        assert_eq!(model.get_outlier_filter_value(), 2.0);
    }

    #[test]
    fn test_get_poincare_points() {
        let model = create_test_model();
        assert!(model.get_poincare_points().is_empty());
    }

    #[test]
    fn test_add_measurement() {
        let mut model = create_test_model();
        let new_msg = create_test_hr_msg(64);
        model.add_measurement(&new_msg).unwrap();
        assert_eq!(model.get_last_msg(), Some(new_msg));
    }

    #[test]
    fn test_set_stats_window() {
        let mut model = create_test_model();
        let new_window = Duration::seconds(120);
        model.set_stats_window(&new_window).unwrap();
        assert_eq!(model.get_stats_window(), &Some(new_window));
    }

    #[test]
    fn test_get_session_data() {
        let model = create_test_model();
        assert!(model.get_session_data().hrv_stats.is_none());
    }

    #[test]
    fn test_get_messages() {
        let model = create_test_model();
        assert_eq!(model.get_messages().len(), 2);
    }

    #[test]
    fn test_get_elapsed_time() {
        let model = create_test_model();
        assert_eq!(model.get_elapsed_time(), Duration::seconds(2));
    }
}
