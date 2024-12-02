//! Bluetooth View
//!
//! This module provides the view layer for managing Bluetooth interactions in the HRV analysis tool.
//! It includes structures and methods for rendering the Bluetooth device selector and interaction UI.

use eframe::egui;
use log::error;
use tokio::sync::mpsc::Sender;
use std::{marker::PhantomData, sync::Arc};

use crate::{
    core::{events::AppEvent, view_trait::ViewApi},
    model::bluetooth::{AdapterHandle, BluetoothModelApi},
};

/// The `BluetoothView` renders a UI for selecting Bluetooth adapters and devices.
///
/// Represents the view for managing Bluetooth interactions, such as device selection and connection.
pub struct ModelInitVView<AHT: AdapterHandle> {
    /// The shared Bluetooth model that provides adapter and device information.
    model: Arc<tokio::sync::Mutex<dyn BluetoothModelApi<AHT>>>,
    event_ch: Sender<AppEvent>,
    /// A marker to track the generic type for the adapter handle.
    _marker: PhantomData<AHT>,
}

impl<AHT: AdapterHandle> ModelInitVView<AHT> {
    /// Creates a new `BluetoothView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the `BluetoothModel`.
    pub fn new(model: Arc<tokio::sync::Mutex<dyn BluetoothModelApi<AHT>>>, event_ch:Sender<AppEvent>) -> Self {
        Self {
            model,
            event_ch,
            _marker: Default::default(),
        }
    }

}

impl<AHT: AdapterHandle + Send + 'static> ViewApi for ModelInitVView<AHT> {

  fn event(&self, event: AppEvent) {
      if let Err(e)= self.event_ch.try_send(event){
        error!("Failed to send AppEvent: {}", e);
      }
  }
    /// Renders the complete Bluetooth view UI.
    ///
    /// # Arguments
    /// * `ctx` - The egui context for rendering the UI.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by user interactions.
    fn render(&self, ctx: &egui::Context)->Result<(), String>{
      egui::CentralPanel::default().show(ctx, |ui|{
        ui.horizontal(|ui|{
          if ui.button("new file").clicked(){
            // new Model
            
          }else if ui.button("load file").clicked(){
            // load model from file
            
          }else{
            
          }
          
        }).inner
        
      });
      Ok(())
    }
}
