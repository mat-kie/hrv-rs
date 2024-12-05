//! Core View Trait
//!
//! This module defines the `ViewTrait`, which is implemented by all views in the HRV analysis tool.
//! It provides a standardized interface for rendering and updating views.




use super::events::UiInputEvent;

/// Trait defining the interface for application views.
///
/// This trait ensures that all views implement methods for rendering and updates.
pub trait ViewApi: Send {

    /// Renders the view and returns an optional event based on user interactions.
    ///
    /// # Arguments
    /// * `ctx` - The `egui::Context` for rendering the UI.
    ///
    /// # Returns
    /// An optional `AppEvent` if the view triggers an action.
    fn render<F: Fn(UiInputEvent) + ?Sized>(&mut self, publish: &F, ctx: &egui::Context)->Result<(), String>;

    
}
