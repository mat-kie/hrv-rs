//! HRV Analysis View
//!
//! This module provides the view layer for visualizing HRV (Heart Rate Variability) analysis results.
//! It includes structures and methods for rendering statistical data, charts, and user interface components.

use crate::{
    core::{
        events::{AppEvent, HrvEvent},
        view_trait::ViewApi,
    },
    model::acquisition::AcquisitionModelApi,
};
use eframe::egui;
use egui::Color32;
use egui_plot::{Legend, Plot, Points};
use log::{error, info};
use std::{ops::RangeInclusive, sync::Arc};
use time::Duration;
use tokio::sync::mpsc::Sender;
use tokio::sync::Mutex;

/// `HrvView` structure.
///
/// Represents the view for visualizing HRV analysis results, including statistics and charts.
pub struct HrvView {
    /// Shared access to the runtime HRV data model.
    model: Arc<Mutex<Box<dyn AcquisitionModelApi>>>,
    event_ch: Sender<AppEvent>,
}

pub fn render_stats(ui: &mut egui::Ui, model: &dyn AcquisitionModelApi, hr: f64) {
    ui.heading("Statistics");
    egui::Grid::new("stats grid").num_columns(2).show(ui, |ui| {
        let desc = egui::Label::new("Heartrate: ");
        ui.add(desc);
        let val = egui::Label::new(format!("{:.2} BPM", hr));
        ui.add(val);
        ui.end_row();

        let desc = egui::Label::new("Elapsed time: ");
        ui.add(desc);
        let val = egui::Label::new(format!("{} s", model.get_elapsed_time().whole_seconds()));
        ui.add(val);
        ui.end_row();

        if let Some(stats) = model.get_hrv_stats() {
            let desc = egui::Label::new("RMSSD [ms]");
            ui.add(desc);
            let val = egui::Label::new(format!("{:.2} ms", stats.rmssd));
            ui.add(val);
            ui.end_row();
            let desc = egui::Label::new("SDRR [ms]");
            ui.add(desc);
            let val = egui::Label::new(format!("{:.2} ms", stats.sdrr));
            ui.add(val);
            ui.end_row();
            let desc = egui::Label::new("SD1 [ms]");
            ui.add(desc);
            let val = egui::Label::new(format!("{:.2} ms", stats.sd1));
            ui.add(val);
            ui.end_row();
            let desc = egui::Label::new("SD2 [ms]");
            ui.add(desc);
            let val = egui::Label::new(format!("{:.2} ms", stats.sd2));
            ui.add(val);
            ui.end_row();
        }
    });
    ui.separator();
}

pub fn render_poincare_plot(ui: &mut egui::Ui, model: &dyn AcquisitionModelApi) {
    let plot = if ui.available_height() < ui.available_width() {
        Plot::new("Poincare Plot")
            .legend(Legend::default())
            .view_aspect(1.0)
            .height(ui.available_height())
    } else {
        Plot::new("Poincare Plot")
            .legend(Legend::default())
            .view_aspect(1.0)
            .width(ui.available_width())
    };

    plot.show(ui, |plot_ui| {
        plot_ui.points(
            Points::new(model.get_poincare_points())
                .name("R-R Intervals")
                .shape(egui_plot::MarkerShape::Diamond)
                .color(Color32::RED)
                .radius(5.0),
        );
    });
}

impl HrvView {
    /// Creates a new `HrvView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the runtime HRV data.
    ///
    /// # Returns
    /// A new `HrvView` instance.
    pub fn new(
        model: Arc<Mutex<Box<dyn AcquisitionModelApi>>>,
        event_ch: Sender<AppEvent>,
    ) -> Self {
        Self { model, event_ch }
    }

    fn render_settings(&self, model: &dyn AcquisitionModelApi, ui: &mut egui::Ui) {
        ui.heading("Settings");
        egui::Grid::new("a grid").num_columns(2).show(ui, |ui| {
            let mut seconds = model
                .get_stats_window()
                .unwrap_or(Duration::minutes(5))
                .as_seconds_f64();
            let desc = egui::Label::new("time window [s]");
            ui.add(desc);
            let slider = egui::Slider::new(&mut seconds, RangeInclusive::new(0.0, 600.0));
            if ui.add(slider).changed() {
                if let Some(new_duration) = Duration::checked_seconds_f64(seconds) {
                    info!("changed value to: {}", seconds);
                    self.event(AppEvent::Data(
                        crate::core::events::HrvEvent::TimeWindowChanged(new_duration),
                    ));
                }
            }
            ui.end_row();
            let mut outlier_value = model.get_outlier_filter_value();
            let desc = egui::Label::new("outlier filter");
            ui.add(desc);
            let slider = egui::Slider::new(&mut outlier_value, RangeInclusive::new(0.0, 10.0));
            if ui.add(slider).changed() {
                info!("changed value to: {}", outlier_value);
                self.event(AppEvent::Data(
                    crate::core::events::HrvEvent::OutlierFilterChanged(outlier_value),
                ));
            }
            ui.end_row();
        });
        ui.separator();
    }

    fn render_acq(&self, ui: &mut egui::Ui) {
        ui.heading("Acquisition");
        egui::Grid::new("acq grid").num_columns(2).show(ui, |ui| {
            if ui.button("Restart").clicked() {
                self.event(AppEvent::DiscardAcquisition);
                self.event(AppEvent::NewAcquisition);
                self.event(AppEvent::Data(HrvEvent::AcquisitionStartReq));
            }
            if ui.button("Stop & Save").clicked() {
                self.event(AppEvent::Data(HrvEvent::AcquisitionStopReq));
                self.event(AppEvent::StoreAcquisition);
            }
            ui.end_row();
        });
        ui.separator();
    }
}

impl ViewApi for HrvView {
    fn event(&self, event: AppEvent) {
        if let Err(e) = self.event_ch.try_send(event) {
            error!("Failed to send AppEvent: {}", e);
        }
    }
    /// Renders the complete HRV analysis view.
    ///
    /// Displays both the HRV statistics panel and the Poincare plot.
    ///
    /// # Arguments
    /// * `ctx` - The egui context for rendering the UI.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by user interactions.
    fn render(&mut self, ctx: &egui::Context) -> Result<(), String> {
        // Extract HRV statistics and Poincare plot points from the model.

        // Render the left panel with HRV statistics.
        let model = self.model.blocking_lock();
        egui::SidePanel::left("left_sidebar").show(ctx, |ui| {
            let msg = model.get_last_msg();
            self.render_settings(&**model, ui);
            if let Some(msg) = msg {
                render_stats(ui, &**model, msg.get_hr());
            }
            self.render_acq(ui);
        });

        // Render the central panel with the Poincare plot.
        egui::CentralPanel::default().show(ctx, |ui| {
            render_poincare_plot(ui, &**model);
        });

        Ok(()) // no errors
    }
}
