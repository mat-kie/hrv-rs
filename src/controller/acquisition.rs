//! Data Acquisition Controller
//!
//! This module defines the controller responsible for managing data acquisition from BLE devices.
//! It interacts with the acquisition model and coordinates data flow during HRV analysis.

use std::{future::Future, path::PathBuf, pin::Pin, sync::Arc};

use crate::{core::events::HrvEvent, model::acquisition::AcquisitionModelApi};
use tokio::sync::Mutex;

/// The `DataAcquisitionApi` trait defines the interface for controlling data acquisition.
/// It provides methods for starting, storing, and discarding acquisitions, as well as handling events.
pub trait DataAcquisitionApi {
    /// Sets the model for the controller
    fn set_model(&mut self, model: Arc<Mutex<dyn AcquisitionModelApi>>);
    /// Starts a new acquisition session.
    fn new_acquisition(&mut self);

    /// Stores the current acquisition session in the model.
    ///
    /// # Returns
    /// `Ok(())` if the operation succeeds, or an `Err(String)` if an error occurs.
    fn store_acquisition(&mut self, path: PathBuf) -> Result<(), String>;

    /// Discards the current acquisition session.
    #[allow(dead_code)]
    fn discard_acquisition(&mut self);

    /// Handles an incoming HRV event and updates the model accordingly.
    ///
    /// # Arguments
    /// * `event` - The `HrvEvent` to process.
    ///
    /// # Returns
    /// `Ok(())` if the event is handled successfully, or an `Err(String)` if an error occurs.
    fn handle_event<'a>(
        &'a mut self,
         event: HrvEvent
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;


}

/// The `AcquisitionController` struct implements the `DataAcquisitionApi` trait and manages
/// data acquisition sessions through an associated model.
///
/// # Type Parameters
/// * `AMT` - A type that implements the `AcquisitionModelApi` trait, representing the underlying data model.
pub struct AcquisitionController {
    /// A thread-safe, shared reference to the acquisition model.
    model: Arc<Mutex<dyn AcquisitionModelApi>>,
}

impl AcquisitionController {
    /// Creates a new `AcquisitionController` instance.
    ///
    /// # Arguments
    /// * `model` - An `Arc<Mutex<AMT>>` representing the thread-safe shared model.
    ///
    /// # Returns
    /// A new instance of `AcquisitionController`.
    pub fn new(model: Arc<Mutex<dyn AcquisitionModelApi>>) -> Self {
        Self { model }
    }
}

impl DataAcquisitionApi for AcquisitionController {
    fn set_model(&mut self, model: Arc<Mutex<dyn AcquisitionModelApi>>) {
        self.model = model
    }
    fn new_acquisition(&mut self) {
        // TODO: Implement logic to initiate a new acquisition.
    }

    fn store_acquisition(&mut self, path: PathBuf) -> Result<(), String> {
        Ok(())
    }

    fn discard_acquisition(&mut self) {
        // self.model.lock().unwrap().discard_acquisition();
    }

    

    fn handle_event<'a>(&'a mut self, event: HrvEvent) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move{

            match event {
                HrvEvent::HrMessage(msg) => {
                    self.model.lock().await.add_measurement(&msg);
                }
                HrvEvent::TimeWindowChanged(time) => {
                    self.model.lock().await.set_stats_window(&time);
                }
                HrvEvent::OutlierFilterChanged(val) => {
                    // TODO: Implement outlier filter update logic.
                    self.model.lock().await.set_outlier_filter_value(val);
                }
            }
            Ok(())
        })
    }
}
