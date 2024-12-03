//! HRV Analysis View
//!
//! This module provides the view layer for visualizing HRV (Heart Rate Variability) analysis results.
//! It includes structures and methods for rendering statistical data, charts, and user interface components.

use crate::{
    core::{events::AppEvent, view_trait::ViewApi},
    model::{acquisition::AcquisitionModelApi, bluetooth::HeartrateMessage},
};
use eframe::egui;
use egui::Color32;
use egui_plot::{Legend, Plot, Points};
use log::{error, info};
use std::{
    ops::RangeInclusive,
    sync::{Arc, Mutex},
};
use time::{Duration, OffsetDateTime};
use tokio::sync::mpsc::Sender;

/// `HrvView` structure.
///
/// Represents the view for visualizing HRV analysis results, including statistics and charts.
pub struct HrvView {
    /// Shared access to the runtime HRV data model.
    model: Arc<Mutex<dyn AcquisitionModelApi>>,
    event_ch: Sender<AppEvent>,
}

impl HrvView {
    /// Creates a new `HrvView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the runtime HRV data.
    ///
    /// # Returns
    /// A new `HrvView` instance.
    pub fn new(model: Arc<Mutex<dyn AcquisitionModelApi>>, event_ch: Sender<AppEvent>) -> Self {
        Self { model, event_ch }
    }

    /// Renders the HRV statistics panel.
    ///
    /// Displays computed HRV metrics, such as heart rate, rMSSD, SD1, and SD2.
    ///
    /// # Arguments
    /// * `ui` - The `egui::Ui` instance for rendering.
    /// * `stats` - Optional HRV statistics to display.
    fn render_statistics(
        &self,
        ui: &mut egui::Ui,
        model: &dyn AcquisitionModelApi,
        msg: &HeartrateMessage,
    ) {
        ui.heading("Statistics");
        egui::Grid::new("stats grid").num_columns(2).show(ui, |ui| {
            let desc = egui::Label::new("Heartrate: ");
            ui.add(desc);
            let val = egui::Label::new(format!("{:.2} BPM", msg.get_hr()));
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

    fn render_settings(
        &self,
        model: &dyn AcquisitionModelApi,
        ui: &mut egui::Ui,
    ){
        ui.heading("Settings");
        egui::Grid::new("a grid")
            .num_columns(2)
            .show(ui, |ui| {
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

    fn render_acq(&self, model: &dyn AcquisitionModelApi, ui: &mut egui::Ui){
        ui.heading("Acquisition");
        egui::Grid::new("acq grid")
            .num_columns(2)
            .show(ui, |ui| {
                let desc = egui::Label::new("Elapsed time: ");
                ui.add(desc);
                let val = egui::Label::new(format!(
                    "{} s",
                    model
                        .get_start_time()
                        .map(|o| { (OffsetDateTime::now_utc() - o).whole_seconds() })
                        .unwrap_or(0)
                ));
                ui.add(val);
                ui.end_row();
                if ui.button("Restart").clicked() {
                    self.event(AppEvent::AcquisitionStartReq);
                }
                if ui.button("Stop & Save").clicked() {
                    let selected = rfd::FileDialog::new().save_file();
                    if let Some(path) = selected {
                        self.event(AppEvent::AcquisitionStopReq(path));
                    }
                }
                ui.end_row();
                
            });
        ui.separator();
    }

    /// Renders the Poincare plot.
    ///
    /// Displays a scatter plot of RR interval data to visualize short- and long-term HRV.
    ///
    /// # Arguments
    /// * `ui` - The `egui::Ui` instance for rendering.
    /// * `points` - The Poincare plot points to display.
    fn render_poincare_plot(&self, ui: &mut egui::Ui, points: &[[f64; 2]]) {
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
                Points::new(points.to_owned())
                    .name("R-R Intervals")
                    .shape(egui_plot::MarkerShape::Diamond)
                    .color(Color32::RED)
                    .radius(5.0),
            );
        });
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
    fn render(&self, ctx: &egui::Context) -> Result<(), String> {
        // Extract HRV statistics and Poincare plot points from the model.

        // Render the left panel with HRV statistics.
        let model = self.model.lock().unwrap();
        let evt = egui::SidePanel::left("left_sidebar")
            .show(ctx, |ui| {
                let msg = model.get_last_msg();
                let evt = { self.render_settings(&*model, ui) };
                if let Some(msg) = msg {
                    self.render_statistics(ui, &*model, &msg);
                }
                self.render_acq(&*model, ui);
                evt
            })
            .inner;

        // Render the central panel with the Poincare plot.
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_poincare_plot(ui, &model.get_poincare_points());
        });

        Ok(()) // no errors
    }
}
