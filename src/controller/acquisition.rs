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
    fn get_acquisition(&self)-> Arc<Mutex<dyn AcquisitionModelApi>>;
    /// Starts a new acquisition session.
    fn reset_acquisition<'a>(
        &'a self
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;

    fn start_acquisition(&mut self);
    fn stop_acquisition(&mut self);


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
    acquiring: bool
}

impl AcquisitionController {
    /// Creates a new `AcquisitionController` instance.
    ///
    /// # Arguments
    /// * `model` - An `Arc<Mutex<AMT>>` representing the thread-safe shared model.
    ///
    /// # Returns
    /// A new instance of `AcquisitionController`.
    pub fn new<AC:AcquisitionModelApi + Default + 'static>() -> Self {
        let  model: Arc<Mutex<dyn AcquisitionModelApi>> = Arc::new(Mutex::new(AC::default()));
        Self { model, acquiring:false }
    }
}

impl DataAcquisitionApi for AcquisitionController {
    fn get_acquisition(&self)-> Arc<Mutex<dyn AcquisitionModelApi>> {
        self.model.clone()
    }
    fn reset_acquisition<'a>(
            &'a self
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move{
            self.model.lock().await.reset();
            Ok(())
        })
    }
fn start_acquisition(&mut self) {
    self.acquiring = true;
}    
fn stop_acquisition(&mut self) {
    self.acquiring = false;
}

    fn handle_event<'a>(&'a mut self, event: HrvEvent) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move{

            match event {
                HrvEvent::HrMessage(msg) => {
                    if self.acquiring{
                        self.model.lock().await.add_measurement(&msg);
                    }
                }
                HrvEvent::TimeWindowChanged(time) => {
                    self.model.lock().await.set_stats_window(&time);
                }
                HrvEvent::OutlierFilterChanged(val) => {
                    // TODO: Implement outlier filter update logic.
                    self.model.lock().await.set_outlier_filter_value(val);
                }
                HrvEvent::AcquisitionStartReq=>{
                    self.acquiring = true
                }
                HrvEvent::AcquisitionStopReq=>{
                    self.acquiring = false
                }
            }
            Ok(())
        })
    }
}
