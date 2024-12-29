use crate::{
    api::{
        controller::{MeasurementApi, OutlierFilter, RecordingApi},
        model::MeasurementModelApi,
    },
    model::{bluetooth::HeartrateMessage, hrv::HrvAnalysisData},
};
use anyhow::Result;
use async_trait::async_trait;
use log::warn;
use serde::{Deserialize, Deserializer, Serialize};
use std::fmt::Debug;
use time::{Duration, OffsetDateTime};

/// Represents the acquisition model, managing HRV-related data and operations.
#[derive(Serialize, Debug, Clone)]
pub struct MeasurementData {
    /// The start time of the acquisition.
    start_time: OffsetDateTime,
    /// Collected measurements with their elapsed time.
    measurements: Vec<(Duration, HeartrateMessage)>,
    /// Window duration for statistical calculations.
    window: Option<usize>,
    /// Outlier filter threshold.
    outlier_filter: f64,
    /// Processed session data.
    #[serde(skip)]
    sessiondata: HrvAnalysisData,
    #[serde(skip)]
    is_recording: bool,
}

impl MeasurementData {
    /// Updates the session data based on the current measurements.
    ///
    /// # Returns
    /// A result indicating success or failure.
    fn update(&mut self) -> Result<()> {
        match HrvAnalysisData::from_acquisition(
            &self.measurements,
            self.window,
            self.outlier_filter,
        ) {
            Ok(data) => self.sessiondata = data,
            Err(e) => {
                warn!("could not calculate session data: {}", e);
            }
        }
        Ok(())
    }
}

impl Default for MeasurementData {
    fn default() -> Self {
        Self {
            start_time: OffsetDateTime::now_utc(),
            measurements: Vec::new(),
            window: None,
            outlier_filter: 5.0,
            sessiondata: Default::default(),
            is_recording: false,
        }
    }
}

impl<'de> Deserialize<'de> for MeasurementData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct AcquisitionModelHelper {
            start_time: OffsetDateTime,
            measurements: Vec<(Duration, HeartrateMessage)>,
            window: Option<usize>,
            outlier_filter: f64,
        }
        // Deserialize all fields except `sessiondata`
        let helper = AcquisitionModelHelper::deserialize(deserializer)?;

        // Reconstruct `sessiondata` from the `measurements`
        let sessiondata = HrvAnalysisData::from_acquisition(
            &helper.measurements,
            helper.window,
            helper.outlier_filter,
        )
        .map_err(serde::de::Error::custom)?;

        Ok(MeasurementData {
            start_time: helper.start_time,
            measurements: helper.measurements,
            window: helper.window,
            outlier_filter: helper.outlier_filter,
            sessiondata,
            is_recording: false,
        })
    }
}

#[async_trait]
impl MeasurementApi for MeasurementData {
    async fn set_stats_window(&mut self, window: usize) -> Result<()> {
        self.window = Some(window);
        self.update()
    }
    async fn set_outlier_filter(&mut self, filter: OutlierFilter) -> Result<()> {
        match filter {
            OutlierFilter::MovingMAD {
                parameter,
                _window: _,
            } => {
                self.outlier_filter = parameter;
            }
        }
        self.update()
    }
    async fn record_message(&mut self, msg: HeartrateMessage) -> Result<()> {
        if self.is_recording {
            let elapsed = OffsetDateTime::now_utc() - self.start_time;
            self.measurements.push((elapsed, msg));
            self.sessiondata
                .add_measurement(&msg, self.window.unwrap_or(usize::MAX))
        } else {
            Err(anyhow::anyhow!(
                "RecordMessage event received while not recording"
            ))
        }
    }
}

impl MeasurementModelApi for MeasurementData {
    fn get_elapsed_time(&self) -> Duration {
        if let Some((elapsed, _)) = self.measurements.last() {
            *elapsed
        } else {
            Duration::default()
        }
    }
    fn get_last_msg(&self) -> Option<&HeartrateMessage> {
        self.measurements.last().map(|(_, msg)| msg)
    }

    fn get_outlier_filter_value(&self) -> f64 {
        self.outlier_filter
    }
    fn get_poincare_points(&self) -> Result<(Vec<[f64; 2]>, Vec<[f64; 2]>)> {
        self.sessiondata.get_poincare(self.window)
    }

    fn get_start_time(&self) -> &OffsetDateTime {
        &self.start_time
    }
    fn get_stats_window(&self) -> Option<usize> {
        self.window
    }
    fn get_dfa1a(&self) -> Option<f64> {
        self.sessiondata.get_dfa_alpha()
    }
    fn get_dfa1a_ts(&self) -> Vec<[f64; 2]> {
        self.sessiondata.get_dfa_alpha_ts().to_owned()
    }
    fn get_hr(&self) -> Option<f64> {
        self.sessiondata.get_hr()
    }
    fn get_hr_ts(&self) -> Vec<[f64; 2]> {
        self.sessiondata.get_hr_ts().to_owned()
    }
    fn get_rmssd(&self) -> Option<f64> {
        self.sessiondata.get_rmssd()
    }
    fn get_rmssd_ts(&self) -> Vec<[f64; 2]> {
        self.sessiondata.get_rmssd_ts().to_owned()
    }
    fn get_sd1(&self) -> Option<f64> {
        self.sessiondata.get_sd1()
    }
    fn get_sd1_ts(&self) -> Vec<[f64; 2]> {
        self.sessiondata.get_sd1_ts().to_owned()
    }
    fn get_sd2(&self) -> Option<f64> {
        self.sessiondata.get_sd2()
    }
    fn get_sd2_ts(&self) -> Vec<[f64; 2]> {
        self.sessiondata.get_sd2_ts().to_owned()
    }
    fn get_sdrr(&self) -> Option<f64> {
        self.sessiondata.get_sdrr()
    }
    fn get_sdrr_ts(&self) -> Vec<[f64; 2]> {
        self.sessiondata.get_sdrr_ts().to_owned()
    }
}

#[async_trait]
impl RecordingApi for MeasurementData {
    async fn start_recording(&mut self) -> Result<()> {
        self.is_recording = true;
        Ok(())
    }

    async fn stop_recording(&mut self) -> Result<()> {
        self.is_recording = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;
    use crate::model::bluetooth::HeartrateMessage;
    #[test]
    fn test_default_measurement_data() {
        let data = MeasurementData::default();
        assert!(data.measurements.is_empty());
        assert_eq!(data.outlier_filter, 100.0);
        assert!(data.window.is_none());
    }

    #[test]
    fn test_update_session_data() {
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        let mut data = MeasurementData::default();
        for _i in 0..4 {
            data.measurements.push((Duration::seconds(1), hr_msg));
        }
        assert!(data.update().is_ok());
    }

    #[test]
    fn test_deserialize_measurement_data() {
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        let mut data = MeasurementData::default();
        for _i in 0..100 {
            data.measurements.push((Duration::seconds(1), hr_msg));
        }
        data.start_time = datetime!(2023-01-01 00:00:00 UTC);
        data.outlier_filter = 100.0;
        let json = serde_json::to_string(&data).unwrap();
        let data: MeasurementData = serde_json::from_str(&json).unwrap();
        assert_eq!(data.start_time, datetime!(2023-01-01 00:00:00 UTC));
        assert_eq!(data.measurements.len(), 100);
        assert_eq!(data.measurements[0].1.get_hr(), 80.0);
        assert_eq!(data.outlier_filter, 100.0);
    }

    #[tokio::test]
    async fn test_set_stats_window() {
        let mut data = MeasurementData::default();
        let window = 60;
        assert!(data.set_stats_window(window).await.is_ok());
        assert_eq!(data.window, Some(window));
    }

    #[tokio::test]
    async fn test_set_outlier_filter() {
        let mut data = MeasurementData::default();
        let filter = OutlierFilter::MovingMAD {
            parameter: 50.0,
            _window: 5,
        };
        assert!(data.set_outlier_filter(filter).await.is_ok());
        assert_eq!(data.outlier_filter, 50.0);
    }

    #[tokio::test]
    async fn test_record_message() {
        let mut data = MeasurementData::default();
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        assert!(data.record_message(hr_msg).await.is_err());
        assert_eq!(data.measurements.len(), 0);
        assert!(data.start_recording().await.is_ok());
        assert!(data.record_message(hr_msg).await.is_ok());
        assert_eq!(data.measurements.len(), 1);
        assert_eq!(data.measurements[0].1.get_hr(), 80.0);
    }

    #[test]
    fn test_get_elapsed_time() {
        let mut data = MeasurementData::default();
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        data.measurements.push((Duration::seconds(1), hr_msg));
        assert_eq!(data.get_elapsed_time(), Duration::seconds(1));
    }

    #[test]
    fn test_get_last_msg() {
        let mut data = MeasurementData::default();
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        data.measurements.push((Duration::seconds(1), hr_msg));
        assert_eq!(data.get_last_msg(), Some(&hr_msg));
    }

    #[test]
    fn test_get_hrv_stats() {
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        let mut data = MeasurementData::default();
        for _i in 0..4 {
            data.measurements.push((Duration::seconds(1), hr_msg));
        }
        data.update().unwrap();
    }

    #[test]
    fn test_get_outlier_filter_value() {
        let data = MeasurementData::default();
        assert_eq!(data.get_outlier_filter_value(), 100.0);
    }

    #[test]
    fn test_get_poincare_points() {
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        let mut data = MeasurementData::default();
        for _i in 0..4 {
            data.measurements.push((Duration::seconds(1), hr_msg));
        }
        data.update().unwrap();
    }

    #[test]
    fn test_get_start_time() {
        let data = MeasurementData::default();
        assert!(data.get_start_time() <= &OffsetDateTime::now_utc());
    }

    #[tokio::test]
    async fn test_get_stats_window() {
        let mut data = MeasurementData::default();
        assert!(data.get_stats_window().is_none());
        assert!(data.set_stats_window(60).await.is_ok());
        assert!(data.get_stats_window().is_some());
        assert_eq!(data.get_stats_window().unwrap(), 60);
    }
}
