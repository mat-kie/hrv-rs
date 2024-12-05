//! Bluetooth View
//!
//! This module provides the view layer for managing Bluetooth interactions in the HRV analysis tool.
//! It includes structures and methods for rendering the Bluetooth device selector and interaction UI.

use crate::{
    core::{events::UiInputEvent, view_trait::ViewApi},
    model::{
        acquisition::AcquisitionModelApi,
        storage::{ModelHandle, StorageModelApi},
    },
};
use time::macros::format_description;

use super::acquisition::{
    render_filter_params, render_poincare_plot, render_stats, render_time_series,
};

/// The `BluetoothView` renders a UI for selecting Bluetooth adapters and devices.
///
/// Represents the view for managing Bluetooth interactions, such as device selection and connection.
pub struct StorageView<SM: StorageModelApi + Send> {
    /// The shared Bluetooth model that provides adapter and device information.
    model: ModelHandle<SM>,
    selected: Option<ModelHandle<dyn AcquisitionModelApi>>,
}

impl<SM: StorageModelApi + Send> StorageView<SM> {
    pub fn new(model: ModelHandle<SM>) -> Self {
        Self {
            model,
            selected: None,
        }
    }
}
impl<SM: StorageModelApi + Send> ViewApi for StorageView<SM> {
    fn render<F: Fn(UiInputEvent) + ?Sized>(
        &mut self,
        publish: &F,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                // "File" Menu
                ui.menu_button("File", |ui| {
                    if ui.button("Open").clicked() {
                        if let Some(file) = rfd::FileDialog::new().pick_file() {
                            publish(UiInputEvent::LoadModel(file));
                        }
                        ui.close_menu(); // Close the menu after selection
                    }
                    if ui.button("Save").clicked() {
                        if let Some(file) = rfd::FileDialog::new().save_file() {
                            publish(UiInputEvent::StoreModel(file));
                        }
                        ui.close_menu();
                    }
                    if ui.button("New").clicked() {
                        publish(UiInputEvent::NewModel);
                        ui.close_menu();
                    }
                });
            });
        });

        let fd = format_description!("[year]-[month]-[day] [hour]:[minute]");
        egui::SidePanel::left("left_overview").show(ctx, |ui| {
            let model = self.model.blocking_read();
            ui.add_space(10.0);
            ui.heading("Past Measurements:");
            for (idx, acq) in model.get_acquisitions().iter().enumerate() {
                let btn = egui::Button::new(
                    acq.blocking_read()
                        .get_start_time()
                        .format(fd)
                        .unwrap()
                        .to_string(),
                );
                if ui.add_sized([ui.available_width(), 20.0], btn).clicked() {
                    self.selected = Some(acq.clone());
                    publish(UiInputEvent::StoredAcqSelected(idx))
                }
            }
            ui.separator();
            if ui.button("New Acquisition").clicked() {
                publish(UiInputEvent::PrepareAcquisition);
            }
        });

        if let Some(selected) = &self.selected {
            egui::SidePanel::right("right:overview").show(ctx, |ui| {
                let model = &*selected.blocking_read();
                let hr = if let Some(stats) = model.get_hrv_stats() {
                    stats.avg_hr
                } else {
                    0.0
                };
                render_stats(ui, model, hr);
                ui.separator();
                render_filter_params(ui, &publish, model);
            });

            egui::TopBottomPanel::bottom("time series panel")
                .min_height(100.0)
                .resizable(true)
                .show(ctx, |ui| {
                    let model = &*selected.blocking_read();
                    render_time_series(ui, model);
                });
            egui::CentralPanel::default().show(ctx, |ui| {
                let model = &*selected.blocking_read();
                render_poincare_plot(ui, model);
            });
        }
        Ok(())
    }
}
