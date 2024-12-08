//! Data Acquisition Controller
//!
//! This module defines the controller responsible for managing data acquisition from BLE devices.
//! It interacts with the acquisition model and coordinates data flow during HRV analysis.

use std::{path::PathBuf, sync::Arc};

use crate::api::{
    controller::{MeasurementApi, RecordingApi, StorageApi, StorageEventApi},
    model::{MeasurementModelApi, ModelHandle, StorageModelApi},
};
use anyhow::{anyhow, Result};

use serde::{de::DeserializeOwned, Serialize};
use tokio::{fs, sync::RwLock};

use async_trait::async_trait;

/// The `AcquisitionController` struct implements the `DataAcquisitionApi` trait and manages
/// data acquisition sessions through an associated model.
///
/// # Type Parameters
/// * `AMT` - A type that implements the `AcquisitionModelApi` trait, representing the underlying data model.
#[derive(Debug, Default)]
pub struct StorageComponent<
    MT: MeasurementApi + DeserializeOwned + Serialize + Default + Send + Sync + Clone + 'static,
> {
    measurements: Vec<Arc<RwLock<MT>>>,
    handles: Vec<ModelHandle<dyn MeasurementModelApi>>,
    active_measurement: Option<Arc<RwLock<MT>>>,
    is_recording: bool,
}

#[async_trait]
impl<
        MT: MeasurementApi + DeserializeOwned + Serialize + Default + Send + Sync + Clone + 'static,
    > StorageEventApi for StorageComponent<MT>
{
    async fn clear(&mut self) -> Result<()> {
        self.measurements.clear();
        self.handles.clear();
        self.active_measurement = None;
        self.is_recording = false;
        Ok(())
    }

    async fn load_from_file(&mut self, path: PathBuf) -> Result<()> {
        let json = fs::read_to_string(&path).await?;
        let measurements = tokio::task::spawn_blocking(move || {
            let serde_result: Result<Vec<MT>, serde_json::Error> =
                serde_json::from_str(json.as_str());
            serde_result
        })
        .await??;
        self.measurements = measurements
            .into_iter()
            .map(|measurement| Arc::new(RwLock::new(measurement)))
            .collect();

        self.handles = self
            .measurements
            .iter()
            .map(|measurement| {
                let mh: ModelHandle<dyn MeasurementModelApi> =
                    ModelHandle::from(measurement.clone());
                mh
            })
            .collect();
        self.active_measurement = None;
        self.is_recording = false;
        Ok(())
    }

    async fn store_to_file(&mut self, path: PathBuf) -> Result<()> {
        let measurements = self.measurements.clone();
        let json = tokio::task::spawn_blocking(move || {
            let guards: Vec<_> = measurements.iter().map(|m| m.blocking_read()).collect();
            let mr: Vec<&MT> = guards.iter().map(|g| &**g).collect();
            serde_json::to_string(&mr)
        })
        .await??;
        fs::write(&path, json).await.map_err(|e| anyhow!(e))
    }

    async fn new_measurement(&mut self) -> Result<()> {
        self.active_measurement = Some(Arc::new(RwLock::new(MT::default())));
        Ok(())
    }

    async fn store_recorded_measurement(&mut self) -> Result<()> {
        if let Some(measurement) = self.active_measurement.take() {
            self.measurements.push(measurement.clone());
            self.handles.push(ModelHandle::from(measurement));
            Ok(())
        } else {
            Err(anyhow!("No active measurement to store"))
        }
    }
}

impl<MT: MeasurementApi + Serialize + DeserializeOwned + Clone + Default> StorageApi<MT>
    for StorageComponent<MT>
{
    fn get_active_measurement(&mut self) -> &Option<Arc<RwLock<MT>>> {
        &self.active_measurement
    }
}

impl<
        MT: MeasurementApi + Serialize + DeserializeOwned + Default + Send + Clone + Sync + 'static,
    > StorageModelApi for StorageComponent<MT>
{
    fn get_acquisitions(&self) -> &[ModelHandle<dyn MeasurementModelApi>] {
        self.handles.as_slice()
    }
}

#[async_trait]
impl<
        MT: MeasurementApi + Serialize + DeserializeOwned + Default + Send + Clone + Sync + 'static,
    > RecordingApi for StorageComponent<MT>
{
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
    use crate::components::measurement::MeasurementData;

    use super::*;

    #[tokio::test]
    async fn test_new_measurement() {
        let mut storage = StorageComponent::<MeasurementData>::default();
        assert!(storage.new_measurement().await.is_ok());
        assert!(storage.get_active_measurement().is_some());
    }

    #[tokio::test]
    async fn test_store_recorded_measurement() {
        let mut storage = StorageComponent::<MeasurementData>::default();
        assert!(storage.new_measurement().await.is_ok());
        assert!(storage.store_recorded_measurement().await.is_ok());
        assert_eq!(storage.get_acquisitions().len(), 1);
    }

    #[tokio::test]
    async fn test_clear_storage() {
        let mut storage = StorageComponent::<MeasurementData>::default();
        assert!(storage.new_measurement().await.is_ok());
        assert!(storage.store_recorded_measurement().await.is_ok());
        assert!(storage.clear().await.is_ok());
        assert_eq!(storage.get_acquisitions().len(), 0);
    }

    #[tokio::test]
    async fn test_store_and_load() {
        let mut storage = StorageComponent::<MeasurementData>::default();
        assert!(storage.new_measurement().await.is_ok());
        assert!(storage.store_recorded_measurement().await.is_ok());

        let path = PathBuf::from("test_measurements.json");
        assert!(storage.store_to_file(path.clone()).await.is_ok());

        let mut new_storage = StorageComponent::<MeasurementData>::default();
        assert!(new_storage.load_from_file(path.clone()).await.is_ok());
        assert_eq!(new_storage.get_acquisitions().len(), 1);

        // Cleanup
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn test_recording_state() {
        let mut storage = StorageComponent::<MeasurementData>::default();
        assert!(storage.start_recording().await.is_ok());
        assert!(storage.stop_recording().await.is_ok());
    }
}
