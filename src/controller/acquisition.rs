//! Data Acquisition Controller
//!
//! This module defines the controller responsible for managing data acquisition from BLE devices.
//! It interacts with the acquisition model and coordinates data flow during HRV analysis.

use std::{sync::Arc, usize};

use crate::{core::events::{AppEvent, BluetoothEvent}, model::{acquisition::AcquisitionModelApi, storage::{ModelHandle, StorageModel, StorageModelApi}}};
use async_trait::async_trait;
use log::error;
use serde::{de::DeserializeOwned, Serialize};
use time::Duration;
use tokio::{sync::{broadcast::{Receiver, Sender}, RwLock}, task::JoinHandle};

/// The `DataAcquisitionApi` trait defines the interface for controlling data acquisition.
/// It provides methods for starting, storing, and discarding acquisitions, as well as handling events.
#[async_trait]
pub trait DataAcquisitionApi {
    /// Start recording an Acquisition, returns the model handle and the sender for the events
    fn start_acquisition(&mut self)->ModelHandle<dyn AcquisitionModelApi>;
    /// stop the current acquisition
    fn stop_acquisition(&mut self);

    fn discard_acquisition(&mut self);
    async fn store_acquisition(&mut self);

    async      fn set_active_acq(&mut self, idx:usize);

    async fn set_stats_window(&mut self, window:&Duration);
    async fn set_outlier_filter_value(&mut self, filter:f64);


}

/// The `AcquisitionController` struct implements the `DataAcquisitionApi` trait and manages
/// data acquisition sessions through an associated model.
///
/// # Type Parameters
/// * `AMT` - A type that implements the `AcquisitionModelApi` trait, representing the underlying data model.
pub struct AcquisitionController<AMT: AcquisitionModelApi + Default> {
    /// A thread-safe, shared reference to the acquisition model.
    model: Arc<RwLock<StorageModel<AMT>>>,
    event_bus: Sender<AppEvent>,
    listener_handle: Option<JoinHandle<()>>,
    active_acquisition: Option<Arc<RwLock<AMT>>>
}

impl< AMT: AcquisitionModelApi + Default> AcquisitionController<AMT> {
    /// Creates a new `AcquisitionController` instance.
    ///
    /// # Arguments
    /// * `model` - An `Arc<Mutex<AMT>>` representing the thread-safe shared model.
    ///
    /// # Returns
    /// A new instance of `AcquisitionController`.
    pub fn new(model: Arc<RwLock<StorageModel<AMT>>>, event_bus: Sender<AppEvent>) -> Self {
        Self { model, event_bus, listener_handle:None, active_acquisition:None}
    }

    async fn msg_listener(acq: Arc<RwLock<AMT>>, mut channel: Receiver<AppEvent>){
        loop {
            match channel.recv().await{
                Ok(AppEvent::Bluetooth(BluetoothEvent::HrMessage(msg)))=>{
                    
                    acq.write().await.add_measurement(&msg);   
                }
                Err(e)=>{ error!("HrMessage receiver terminated: {}",e);
                break;
                }
                _=>{}
            }
        }
        
       
    }
}

#[async_trait]
impl<AMT: AcquisitionModelApi+ Default + Serialize + DeserializeOwned+ 'static> DataAcquisitionApi for AcquisitionController<AMT> {
    async fn set_active_acq(&mut self, idx:usize){
        let model = self.model.read().await;
        if idx < model.get_mut_acquisitions().len(){
            self.active_acquisition = Some(model.get_mut_acquisitions()[idx].clone());
        }

    }
    
    async fn set_stats_window(&mut self, window:&Duration)
    {
        if let Some(acq) = &self.active_acquisition{
            acq.write().await.set_stats_window(window);
        }
        
    }
    
    async fn set_outlier_filter_value(&mut self, filter:f64){

        if let Some(acq) = &self.active_acquisition{
            acq.write().await.set_outlier_filter_value(filter);
        }

    }
fn start_acquisition(&mut self)->ModelHandle<dyn AcquisitionModelApi> {
    let acq:Arc<RwLock<AMT>>  = Arc::new(RwLock::new(AMT::default()));   
    self.active_acquisition = Some(acq.clone());
    let jh = tokio::spawn(Self::msg_listener(acq.clone(), self.event_bus.subscribe()));
    if let Some(jh_old) = self.listener_handle.replace(jh){
        jh_old.abort();
    }
    (acq as Arc<RwLock<dyn AcquisitionModelApi>>).into()
}
fn discard_acquisition(&mut self) {
    self.stop_acquisition();
    self.active_acquisition = None;
}

fn stop_acquisition(&mut self) {
    if let Some(hnd) = self.listener_handle.take(){
        hnd.abort();
    }
}
async fn store_acquisition(&mut self) {
    self.stop_acquisition();
    if let Some(acq) = self.active_acquisition.take(){
        self.model.write().await.store_acquisition(acq);
    }
}
}
