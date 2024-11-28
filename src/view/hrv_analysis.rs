//! HRV Analysis View
//!
//! This module provides the view layer for visualizing HRV (Heart Rate Variability) analysis results.
//! It includes structures and methods for rendering statistical data, charts, and user interface components.

use crate::{
    core::{events::AppEvent, view_trait::ViewApi},
    model::acquisition::AcquisitionModelApi,
    model::hrv::HrvStatistics,
};
use eframe::egui;
use egui::Color32;
use egui_plot::{Legend, Plot, Points};
use std::sync::{Arc, Mutex};

/// `HrvView` structure.
///
/// Represents the view for visualizing HRV analysis results, including statistics and charts.
pub struct HrvView {
    /// Shared access to the runtime HRV data model.
    model: Arc<Mutex<dyn AcquisitionModelApi>>,
}

impl HrvView {
    /// Creates a new `HrvView` instance.
    ///
    /// # Arguments
    /// * `model` - Shared access to the runtime HRV data.
    ///
    /// # Returns
    /// A new `HrvView` instance.
    pub fn new(model: Arc<Mutex<dyn AcquisitionModelApi>>) -> Self {
        Self { model }
    }

    /// Renders the HRV statistics panel.
    ///
    /// Displays computed HRV metrics, such as heart rate, rMSSD, SD1, and SD2.
    ///
    /// # Arguments
    /// * `ui` - The `egui::Ui` instance for rendering.
    /// * `stats` - Optional HRV statistics to display.
    fn render_statistics(&self, ui: &mut egui::Ui, stats: &Option<HrvStatistics>) {
        if let Some(hrv) = stats {
            ui.heading("HRV Statistics");

            ui.horizontal(|ui| {
                let label = ui.label("Heart Rate: ");
                ui.label(format!("{:.1} 1/min", hrv.avg_hr))
                    .labelled_by(label.id);
            });

            ui.horizontal(|ui| {
                let label = ui.label("rMSSD: ");
                ui.label(format!("{:.2} ms", hrv.rmssd))
                    .labelled_by(label.id);
            });

            ui.horizontal(|ui| {
                let label = ui.label("SD1: ");
                ui.label(format!("{:.2} ms", hrv.sd1)).labelled_by(label.id);
            });

            ui.horizontal(|ui| {
                let label = ui.label("SD2: ");
                ui.label(format!("{:.2} ms", hrv.sd2)).labelled_by(label.id);
            });
        } else {
            ui.label("No HRV statistics available.");
        }
    }

    /// Renders the Poincare plot.
    ///
    /// Displays a scatter plot of RR interval data to visualize short- and long-term HRV.
    ///
    /// # Arguments
    /// * `ui` - The `egui::Ui` instance for rendering.
    /// * `points` - The Poincare plot points to display.
    fn render_poincare_plot(&self, ui: &mut egui::Ui, points: &[[f64; 2]]) {
        let plot = Plot::new("Poincare Plot")
            .legend(Legend::default())
            .view_aspect(1.0);

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
    /// Renders the complete HRV analysis view.
    ///
    /// Displays both the HRV statistics panel and the Poincare plot.
    ///
    /// # Arguments
    /// * `ctx` - The egui context for rendering the UI.
    ///
    /// # Returns
    /// An optional `AppEvent` triggered by user interactions.
    fn render(&self, ctx: &egui::Context) -> Option<AppEvent> {
        // Extract HRV statistics and Poincare plot points from the model.
        let (stats, points) = self
            .model
            .lock()
            .map(|model| (model.get_hrv_stats().clone(), model.get_poincare_points()))
            .unwrap_or((None, Vec::new()));

        // Render the left panel with HRV statistics.
        egui::SidePanel::left("left_sidebar").show(ctx, |ui| {
            self.render_statistics(ui, &stats);
        });

        // Render the central panel with the Poincare plot.
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_poincare_plot(ui, &points);
        });

        None // No events to emit from this view.
    }
}
