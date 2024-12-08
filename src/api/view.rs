//! Core View Trait
//!
//! This module defines the `ViewApi` trait, which is implemented by all views in the HRV analysis tool.
//! It provides a standardized interface for rendering and updating views.

use crate::core::events::AppEvent;

/// Trait defining the interface for application views.
///
/// This trait ensures that all views implement methods for rendering and updates.
pub trait ViewApi: Send {
    /// Renders the view and returns an optional event based on user interactions.
    ///
    /// # Arguments
    /// * `publish` - A function to publish `UiInputEvent` events.
    /// * `ctx` - The `egui::Context` for rendering the UI.
    ///
    /// # Returns
    /// A result indicating success or failure.
    fn render<F: Fn(AppEvent) + ?Sized>(
        &mut self,
        publish: &F,
        ctx: &egui::Context,
    ) -> Result<(), String>;
}
