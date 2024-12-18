use crate::{
    api::{
        controller::{MeasurementApi, OutlierFilter},
        model::MeasurementModelApi,
    },
    model::{
        bluetooth::HeartrateMessage,
        hrv::{HrvSessionData, HrvStatistics},
    },
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
    window: Option<Duration>,
    /// Outlier filter threshold.
    outlier_filter: f64,
    /// Processed session data.
    #[serde(skip)]
    sessiondata: HrvSessionData,
}

impl MeasurementData {
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

impl Default for MeasurementData {
    fn default() -> Self {
        Self {
            start_time: OffsetDateTime::now_utc(),
            measurements: Vec::new(),
            window: None,
            outlier_filter: 100.0,
            sessiondata: Default::default(),
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

        Ok(MeasurementData {
            start_time: helper.start_time,
            measurements: helper.measurements,
            window: helper.window,
            outlier_filter: helper.outlier_filter,
            sessiondata,
        })
    }
}

#[async_trait]
impl MeasurementApi for MeasurementData {
    async fn set_stats_window(&mut self, window: Duration) -> Result<()> {
        self.window = Some(window);
        Ok(())
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
        Ok(())
    }
    async fn record_message(&mut self, msg: HeartrateMessage) -> Result<()> {
        let elapsed = OffsetDateTime::now_utc() - self.start_time;
        self.measurements.push((elapsed, msg));
        self.update()
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
    fn get_hrv_stats(&self) -> Option<&HrvStatistics> {
        self.sessiondata.hrv_stats.as_ref()
    }

    fn get_outlier_filter_value(&self) -> f64 {
        self.outlier_filter
    }
    fn get_poincare_points(&self) -> Vec<[f64; 2]> {
        self.sessiondata.get_poincare()
    }
    fn get_session_data(&self) -> &HrvSessionData {
        &self.sessiondata
    }
    fn get_start_time(&self) -> &OffsetDateTime {
        &self.start_time
    }
    fn get_stats_window(&self) -> Option<&Duration> {
        self.window.as_ref()
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
        assert!(data.sessiondata.hrv_stats.is_some());
    }

    #[test]
    fn test_deserialize_measurement_data() {
        let hr_msg = HeartrateMessage::new(&[0b10000, 80, 255, 0]);
        let mut data = MeasurementData::default();
        for _i in 0..4 {
            data.measurements.push((Duration::seconds(1), hr_msg));
        }
        data.start_time = datetime!(2023-01-01 00:00:00 UTC);
        data.outlier_filter = 100.0;
        let json = serde_json::to_string(&data).unwrap();
        let data: MeasurementData = serde_json::from_str(&json).unwrap();
        assert_eq!(data.start_time, datetime!(2023-01-01 00:00:00 UTC));
        assert_eq!(data.measurements.len(), 4);
        assert_eq!(data.measurements[0].1.get_hr(), 80.0);
        assert_eq!(data.outlier_filter, 100.0);
    }

    #[tokio::test]
    async fn test_set_stats_window() {
        let mut data = MeasurementData::default();
        let window = Duration::seconds(60);
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
        assert!(data.get_hrv_stats().is_some());
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
        assert_eq!(data.get_poincare_points().len(), 3);
    }

    #[test]
    fn test_get_session_data() {
        let data = MeasurementData::default();
        assert!(data.get_session_data().hrv_stats.is_none());
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
        assert!(data.set_stats_window(Duration::seconds(60)).await.is_ok());
        assert!(data.get_stats_window().is_some());
        assert_eq!(data.get_stats_window().unwrap(), &Duration::seconds(60));
    }
}
