//! Data Acquisition Controller
//!
//! This module defines the controller responsible for managing data acquisition from BLE devices.
//! It interacts with the acquisition model and coordinates data flow during HRV analysis.

use std::sync::Arc;

use crate::{
    core::events::{AppEvent, BluetoothEvent},
    model::{
        acquisition::AcquisitionModelApi,
        storage::{ModelHandle, StorageModelApi},
    },
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::error;
use time::Duration;
use tokio::{
    sync::{
        broadcast::{Receiver, Sender},
        RwLock,
    },
    task::JoinHandle,
};

/// The `DataAcquisitionApi` trait defines the interface for controlling data acquisition.
/// It provides methods for starting, storing, and discarding acquisitions, as well as handling events.
#[async_trait]
pub trait DataAcquisitionApi {
    /// Start recording an Acquisition, returns the model handle and the sender for the events.
    fn start_acquisition(&mut self) -> Result<ModelHandle<dyn AcquisitionModelApi>>;
    /// Stop the current acquisition.
    fn stop_acquisition(&mut self) -> Result<()>;

    /// Discard the current acquisition.
    fn discard_acquisition(&mut self) -> Result<()>;
    /// Store the current acquisition asynchronously.
    async fn store_acquisition(&mut self) -> Result<()>;

    /// Set the active acquisition by index asynchronously.
    async fn set_active_acq(&mut self, idx: usize) -> Result<()>;

    /// Set the statistics window for the active acquisition asynchronously.
    async fn set_stats_window(&mut self, window: &Duration) -> Result<()>;
    /// Set the outlier filter value for the active acquisition asynchronously.
    async fn set_outlier_filter_value(&mut self, filter: f64) -> Result<()>;
}

/// The `AcquisitionController` struct implements the `DataAcquisitionApi` trait and manages
/// data acquisition sessions through an associated model.
///
/// # Type Parameters
/// * `AMT` - A type that implements the `AcquisitionModelApi` trait, representing the underlying data model.
pub struct AcquisitionController<AMT, SMT>
where
    AMT: AcquisitionModelApi + Default + 'static,
    SMT: StorageModelApi<AcqModelType = AMT> + Send + Sync,
{
    /// A thread-safe, shared reference to the acquisition model.
    model: Arc<RwLock<SMT>>,
    /// A sender for broadcasting application events.
    event_bus: Sender<AppEvent>,
    /// A handle for the listener task.
    listener_handle: Option<JoinHandle<()>>,
    /// The currently active acquisition, if any.
    active_acquisition: Option<Arc<RwLock<AMT>>>,
}

impl<AMT, SMT> AcquisitionController<AMT, SMT>
where
    AMT: AcquisitionModelApi + Default + 'static,
    SMT: StorageModelApi<AcqModelType = AMT> + Send + Sync,
{
    /// Creates a new `AcquisitionController` instance.
    ///
    /// # Arguments
    /// * `model` - An `Arc<RwLock<SMT>>` representing the thread-safe shared model.
    /// * `event_bus` - A `Sender<AppEvent>` for broadcasting application events.
    ///
    /// # Returns
    /// A new instance of `AcquisitionController`.
    pub fn new(model: Arc<RwLock<SMT>>, event_bus: Sender<AppEvent>) -> Self {
        Self {
            model,
            event_bus,
            listener_handle: None,
            active_acquisition: None,
        }
    }

    /// Listens for messages on the event bus and processes them.
    ///
    /// # Arguments
    /// * `acq` - A shared reference to the active acquisition model.
    /// * `channel` - A receiver for application events.
    async fn msg_listener(acq: Arc<RwLock<AMT>>, mut channel: Receiver<AppEvent>) {
        loop {
            match channel.recv().await {
                Ok(AppEvent::Bluetooth(BluetoothEvent::HrMessage(msg))) => {
                    if let Err(e) = acq.write().await.add_measurement(&msg) {
                        error!("failed to add measurement: {}", e);
                    }
                }
                Err(e) => {
                    error!("HrMessage receiver terminated: {}", e);
                    break;
                }
                _ => {}
            }
        }
    }

    /// Retrieves the currently active acquisition.
    ///
    /// # Returns
    /// A reference to the active acquisition, or an error if none is active.
    fn get_active_acq(&self) -> Result<&Arc<RwLock<AMT>>> {
        self.active_acquisition
            .as_ref()
            .ok_or(anyhow!("No active Acquisition present"))
    }
}

#[async_trait]
impl<AMT, SMT> DataAcquisitionApi for AcquisitionController<AMT, SMT>
where
    AMT: AcquisitionModelApi + Default + 'static,
    SMT: StorageModelApi<AcqModelType = AMT> + Send + Sync + 'static,
{
    async fn set_active_acq(&mut self, idx: usize) -> Result<()> {
        let model = self.model.read().await;
        if idx < model.get_mut_acquisitions().len() {
            self.active_acquisition = Some(model.get_mut_acquisitions()[idx].clone());
            Ok(())
        } else {
            Err(anyhow!(
                "requested an out of bounds acquisition index: {}, #acquisitions: {}",
                idx,
                model.get_mut_acquisitions().len()
            ))
        }
    }

    async fn set_stats_window(&mut self, window: &Duration) -> Result<()> {
        self.get_active_acq()?
            .write()
            .await
            .set_stats_window(window)
    }

    async fn set_outlier_filter_value(&mut self, filter: f64) -> Result<()> {
        self.get_active_acq()?
            .write()
            .await
            .set_outlier_filter_value(filter)
    }

    fn start_acquisition(&mut self) -> Result<ModelHandle<dyn AcquisitionModelApi>> {
        let acq: Arc<RwLock<AMT>> = Arc::new(RwLock::new(AMT::default()));
        self.active_acquisition = Some(acq.clone());
        let jh = tokio::spawn(Self::msg_listener(acq.clone(), self.event_bus.subscribe()));
        if let Some(jh_old) = self.listener_handle.replace(jh) {
            jh_old.abort();
        }
        Ok((acq as Arc<RwLock<dyn AcquisitionModelApi>>).into())
    }

    fn discard_acquisition(&mut self) -> Result<()> {
        self.stop_acquisition()?;
        self.active_acquisition = None;
        Ok(())
    }

    fn stop_acquisition(&mut self) -> Result<()> {
        if let Some(hnd) = self.listener_handle.take() {
            hnd.abort();
        }
        Ok(())
    }

    async fn store_acquisition(&mut self) -> Result<()> {
        self.stop_acquisition()?;
        if let Some(acq) = self.active_acquisition.take() {
            self.model.write().await.store_acquisition(acq);
            Ok(())
        } else {
            Err(anyhow!(
                "Tried to store an acquisition while none was active"
            ))
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{acquisition::MockAcquisitionModelApi, storage::MockStorageModelApi};

    use tokio::sync::broadcast;
    #[tokio::test]
    async fn test_start_acquisition() {
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        let mut controller = AcquisitionController::new(model, tx);

        let result = controller.start_acquisition();
        assert!(result.is_ok());
        assert!(controller.active_acquisition.is_some());
    }

    #[tokio::test]
    async fn test_stop_acquisition() {
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        let mut controller = AcquisitionController::new(model, tx);

        controller.start_acquisition().unwrap();
        let result = controller.stop_acquisition();
        assert!(result.is_ok());
        assert!(controller.listener_handle.is_none());
    }

    #[tokio::test]
    async fn test_discard_acquisition() {
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        let mut controller = AcquisitionController::new(model, tx);

        controller.start_acquisition().unwrap();
        let result = controller.discard_acquisition();
        assert!(result.is_ok());
        assert!(controller.active_acquisition.is_none());
    }

    #[tokio::test]
    async fn test_store_acquisition() {
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        model
            .write()
            .await
            .expect_store_acquisition()
            .once()
            .return_const(());
        let mut controller = AcquisitionController::new(model, tx);

        controller.start_acquisition().unwrap();
        let result = controller.store_acquisition().await;
        assert!(result.is_ok());
        assert!(controller.active_acquisition.is_none());
        let result = controller.store_acquisition().await;
        assert!(result.is_err());
        assert!(controller.active_acquisition.is_none());
    }

    #[tokio::test]
    async fn test_set_active_acq() {
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        model
            .write()
            .await
            .expect_get_mut_acquisitions()
            .times(1..)
            .return_const(Vec::default());
        let mut controller = AcquisitionController::new(model, tx);
        controller.start_acquisition().unwrap();
        let result = controller.set_active_acq(0).await;
        assert!(controller.active_acquisition.is_some());
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_active_acq_nonzero() {
        let (tx, _rx) = broadcast::channel(16);
        let vec = vec![Arc::new(RwLock::new(MockAcquisitionModelApi::default()))];
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        model
            .write()
            .await
            .expect_get_mut_acquisitions()
            .times(1..)
            .return_const(vec);
        let mut controller = AcquisitionController::new(model, tx);
        let result = controller.set_active_acq(0).await;
        assert!(result.is_ok());
        assert!(controller.active_acquisition.is_some());
        let result = controller.set_active_acq(1).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_set_stats_window() {
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        let mut controller = AcquisitionController::new(model, tx);

        controller.start_acquisition().unwrap();
        let window = Duration::seconds(60);

        controller
            .get_active_acq()
            .unwrap()
            .write()
            .await
            .expect_set_stats_window()
            .once()
            .returning(|x| {
                assert_eq!(x, &Duration::seconds(60));
                Ok(())
            });

        let result = controller.set_stats_window(&window).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_set_outlier_filter_value() {
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        let mut controller = AcquisitionController::new(model, tx);

        controller.start_acquisition().unwrap();

        controller
            .get_active_acq()
            .unwrap()
            .write()
            .await
            .expect_set_outlier_filter_value()
            .once()
            .returning(|x| {
                assert_eq!(x, 1.5);
                Ok(())
            });

        let result = controller.set_outlier_filter_value(1.5).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_active_acq() {
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        let mut controller = AcquisitionController::new(model, tx);

        controller.start_acquisition().unwrap();
        let result = controller.get_active_acq();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_active_acq_none() {
        let (tx, _rx) = broadcast::channel(16);
        let model = Arc::new(RwLock::new(MockStorageModelApi::default()));
        let controller = AcquisitionController::new(model, tx);

        let result = controller.get_active_acq();
        assert!(result.is_err());
    }
}
