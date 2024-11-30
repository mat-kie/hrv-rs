//! Data Acquisition Controller
//!
//! This module defines the controller responsible for managing data acquisition from BLE devices.
//! It interacts with the acquisition model and coordinates data flow during HRV analysis.

use std::sync::{Arc, Mutex};

use crate::{core::events::HrvEvent, model::acquisition::AcquisitionModelApi};

/// The `DataAcquisitionApi` trait defines the interface for controlling data acquisition.
/// It provides methods for starting, storing, and discarding acquisitions, as well as handling events.
pub trait DataAcquisitionApi {
    /// Starts a new acquisition session.
    fn new_acquisition(&mut self);

    /// Stores the current acquisition session in the model.
    ///
    /// # Returns
    /// `Ok(())` if the operation succeeds, or an `Err(String)` if an error occurs.
    fn store_acquisition(&mut self) -> Result<(), String>;

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
    #[allow(dead_code)]
    fn handle_event(&mut self, event: HrvEvent) -> Result<(), String>;
}

/// The `AcquisitionController` struct implements the `DataAcquisitionApi` trait and manages
/// data acquisition sessions through an associated model.
///
/// # Type Parameters
/// * `AMT` - A type that implements the `AcquisitionModelApi` trait, representing the underlying data model.
pub struct AcquisitionController<AMT: AcquisitionModelApi> {
    /// A thread-safe, shared reference to the acquisition model.
    model: Arc<Mutex<AMT>>,
}

impl<AMT: AcquisitionModelApi> AcquisitionController<AMT> {
    /// Creates a new `AcquisitionController` instance.
    ///
    /// # Arguments
    /// * `model` - An `Arc<Mutex<AMT>>` representing the thread-safe shared model.
    ///
    /// # Returns
    /// A new instance of `AcquisitionController`.
    pub fn new(model: Arc<Mutex<AMT>>) -> Self {
        Self { model }
    }
}

impl<AMT: AcquisitionModelApi> DataAcquisitionApi for AcquisitionController<AMT> {
    fn new_acquisition(&mut self) {
        // TODO: Implement logic to initiate a new acquisition.
    }

    fn store_acquisition(&mut self) -> Result<(), String> {
        self.model.lock().unwrap().store_acquisition();
        Ok(())
    }

    fn discard_acquisition(&mut self) {
        self.model.lock().unwrap().discard_acquisition();
    }

    fn handle_event(&mut self, event: HrvEvent) -> Result<(), String> {
        match event {
            HrvEvent::HrMessage(msg) => {
                self.model.lock().unwrap().add_measurement(&msg);
            }
            HrvEvent::TimeWindowChanged(time) => {
                self.model.lock().unwrap().set_stats_window(&time);
            }
            HrvEvent::OutlierFilterChanged(val) => {
                // TODO: Implement outlier filter update logic.
                self.model.lock().unwrap().set_outlier_filter_value(val);
            }
        }
        Ok(())
    }
}
