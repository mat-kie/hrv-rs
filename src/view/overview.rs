//! Bluetooth View
//!
//! This module provides the view layer for managing Bluetooth interactions in the HRV analysis tool.
//! It includes structures and methods for rendering the Bluetooth device selector and interaction UI.

use crate::{
    core::{events::AppEvent, view_trait::ViewApi},
    model::{
        acquisition::AcquisitionModelApi,
        storage::{StorageModel, StorageModelApi},
    },
};
use log::error;
use std::sync::Arc;
use time::macros::format_description;
use tokio::sync::{mpsc::Sender, Mutex};

use super::hrv_analysis::{render_poincare_plot, render_stats};

/// The `BluetoothView` renders a UI for selecting Bluetooth adapters and devices.
///
/// Represents the view for managing Bluetooth interactions, such as device selection and connection.
pub struct StorageView {
    /// The shared Bluetooth model that provides adapter and device information.
    model: Arc<Mutex<StorageModel>>,
    event_ch: Sender<AppEvent>,
    selected: Option<Arc<Mutex<Box<dyn AcquisitionModelApi>>>>,
}

impl StorageView {
    pub fn new(model: Arc<Mutex<StorageModel>>, event_ch: Sender<AppEvent>) -> Self {
        Self {
            model,
            event_ch,
            selected: None,
        }
    }
}
impl ViewApi for StorageView {
    fn event(&self, event: AppEvent) {
        if let Err(e) = self.event_ch.try_send(event) {
            error!("Failed to send AppEvent: {}", e);
        }
    }

    fn render(&mut self, ctx: &egui::Context) -> Result<(), String> {
        let fd = format_description!("[year]-[month]-[day] [hour]:[minute]");
        egui::SidePanel::left("left_overview").show(ctx, |ui| {
            let model = self.model.blocking_lock();
            for acq in model.get_acquisitions() {
                if ui
                    .button(
                        acq.blocking_lock().get_start_time().format(fd).unwrap().to_string()
                    )
                    .clicked()
                {
                    self.selected = Some(acq.clone());
                }
            }
            ui.separator();
            if ui.button("New Acquisition").clicked() {
                self.event(AppEvent::SelectDevice);
            }
            ui.separator();
            if ui.button("Store database").clicked() {
                if let Some(file) = rfd::FileDialog::new().save_file() {
                    self.event(AppEvent::StoreModel(file));
                }
            }
        });

        if let Some(selected) = &self.selected {
            egui::SidePanel::right("right:overview").show(ctx, |ui| {
                let model = &**selected.blocking_lock();
                let hr = if let Some(stats) = model.get_hrv_stats() {
                    stats.avg_hr
                } else {
                    0.0
                };
                render_stats(ui, model, hr);
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                let model = &**selected.blocking_lock();
                render_poincare_plot(ui, model);
            });
        }
        Ok(())
    }
}
