//! Data Acquisition Controller
//!
//! This module defines the controller responsible for managing data acquisition from BLE devices.
//! It interacts with the acquisition model and coordinates data flow during HRV analysis.

use std::{path::PathBuf, sync::Arc};

use crate::api::{
    controller::{MeasurementApi, StorageApi, StorageEventApi},
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
}

impl<MT: MeasurementApi + Serialize + DeserializeOwned + Clone + Default> StorageApi<MT>
    for StorageComponent<MT>
{
    fn get_measurement(&self, index: usize) -> Result<Arc<RwLock<MT>>> {
        if index < self.measurements.len() {
            Ok(self.measurements[index].clone())
        } else {
            Err(anyhow!("Index out of bounds"))
        }
    }
    fn store_measurement(&mut self, measurement: Arc<RwLock<MT>>) -> Result<()> {
        self.measurements.push(measurement.clone());
        let mh: ModelHandle<dyn MeasurementModelApi> = ModelHandle::from(measurement.clone());
        self.handles.push(mh);
        Ok(())
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

#[cfg(test)]
mod tests {
    use crate::components::measurement::MeasurementData;

    use super::*;

    #[tokio::test]
    async fn test_clear_storage() {
        let mut storage = StorageComponent::<MeasurementData>::default();
        let measurement = Arc::new(RwLock::new(MeasurementData::default()));
        assert!(storage.store_measurement(measurement.clone()).is_ok());
        assert!(storage.clear().await.is_ok());
        assert_eq!(storage.get_acquisitions().len(), 0);
    }

    #[tokio::test]
    async fn test_load_from_nonexistent_file() {
        let temp_dir = tempdir::TempDir::new("test").unwrap();
        let path = temp_dir
            .path()
            .join(PathBuf::from("some/invalid/subdir/nonexistent.json"));
        let mut storage = StorageComponent::<MeasurementData>::default();
        let result = storage.load_from_file(path).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_store_to_invalid_path() {
        let temp_dir = tempdir::TempDir::new("test").unwrap();
        let path = temp_dir
            .path()
            .join(PathBuf::from("some/invalid/subdir/test_measurements.json"));
        let mut storage = StorageComponent::<MeasurementData>::default();
        let measurement = Arc::new(RwLock::new(MeasurementData::default()));
        assert!(storage.store_measurement(measurement.clone()).is_ok());
        let result = storage.store_to_file(path).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_store_and_load() {
        let temp_dir = tempdir::TempDir::new("test").unwrap();
        let path = temp_dir
            .path()
            .join(PathBuf::from("test_measurements.json"));
        let mut storage = StorageComponent::<MeasurementData>::default();
        let measurement = Arc::new(RwLock::new(MeasurementData::default()));
        assert!(storage.store_measurement(measurement.clone()).is_ok());
        assert!(storage.store_to_file(path.clone()).await.is_ok());

        let mut new_storage = StorageComponent::<MeasurementData>::default();
        assert!(new_storage.load_from_file(path.clone()).await.is_ok());
        assert_eq!(new_storage.get_acquisitions().len(), 1);
    }

    #[tokio::test]
    async fn test_get_measurement_out_of_bounds() {
        let storage = StorageComponent::<MeasurementData>::default();
        let result = storage.get_measurement(0);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_store_and_retrieve_measurement() {
        let mut storage = StorageComponent::<MeasurementData>::default();
        let measurement = Arc::new(RwLock::new(MeasurementData::default()));
        assert!(storage.store_measurement(measurement.clone()).is_ok());
        let retrieved = storage.get_measurement(0).unwrap();
        assert!(Arc::ptr_eq(&measurement, &retrieved))
    }
}
