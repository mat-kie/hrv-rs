//! Bluetooth View
//!
//! This module provides the view layer for managing Bluetooth interactions in the HRV analysis tool.
//! It includes structures and methods for rendering the Bluetooth device selector and interaction UI.

use eframe::egui;
use egui::Color32;
use std::{marker::PhantomData, sync::Arc};

use crate::{
    core::{events::AppEvent, events::BluetoothEvent, view_trait::ViewApi},
    model::bluetooth::{AdapterHandle, BluetoothModelApi},
};

/// The `BluetoothView` renders a UI for selecting Bluetooth adapters and devices.
///
/// Represents the view for managing Bluetooth interactions, such as device selection and connection.
pub struct ModelInitVView<AHT: AdapterHandle> {
    /// The shared Bluetooth model that provides adapter and device information.
    model: Arc<tokio::sync::Mutex<dyn BluetoothModelApi<AHT>>>,
    /// A marker to track the generic type for the adapter handle.
    _marker: PhantomData<AHT>,
}

impl<AHT: AdapterHandle> ModelInitVView<AHT> {
    /// Creates a new `BluetoothView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the `BluetoothModel`.
    pub fn new(model: Arc<tokio::sync::Mutex<dyn BluetoothModelApi<AHT>>>) -> Self {
        Self {
            model,
            _marker: Default::default(),
        }
    }

}

impl<AHT: AdapterHandle + Send + 'static> ViewApi for ModelInitVView<AHT> {
    /// Renders the complete Bluetooth view UI.
    ///
    /// # Arguments
    /// * `ctx` - The egui context for rendering the UI.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by user interactions.
    fn render(&self, ctx: &egui::Context) -> Option<AppEvent> {
      egui::CentralPanel::default().show(ctx, |ui|{
        ui.horizontal(|ui|{
          if ui.button("new file").clicked(){
            // new Model
            None
          }else if ui.button("load file").clicked(){
            // load model from file
            None
          }else{
            None
          }
          
        }).inner
        
      }).inner
    }
}
