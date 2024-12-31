/// Constructs a new `HrvStatistics` from RR intervals and heart rate values.
///
/// # Arguments
///
/// * `rr_intervals` - A slice of RR intervals in milliseconds.
/// * `hr_values` - A slice of heart rate values.
///
/// # Returns
///
/// Returns an `Ok(HrvStatistics)` containing the calculated HRV statistics, or
/// an `Err` if there is insufficient data.
use std::sync::Arc;

use eframe::App;
use log::error;
use tokio::{
    sync::{
        broadcast::{Receiver, Sender},
        RwLock,
    },
    task::JoinHandle,
};

use crate::{
    api::{
        model::{BluetoothModelApi, MeasurementModelApi, ModelHandle, StorageModelApi},
        view::ViewApi,
    },
    core::events::AppEvent,
};

use super::{acquisition::AcquisitionView, overview::StorageView};

/// Represents the different states of the application's view.
///
/// This enum is used to switch between the overview and acquisition views.
#[derive(Clone, Debug)]
pub enum ViewState {
    /// The overview view displaying stored acquisitions.
    Overview(
        (
            ModelHandle<dyn StorageModelApi>,
            Option<ModelHandle<dyn MeasurementModelApi>>,
        ),
    ),
    /// The acquisition view for real-time data collection.
    Acquisition(
        (
            ModelHandle<dyn MeasurementModelApi>,
            ModelHandle<dyn BluetoothModelApi>,
        ),
    ),
}

/// Enumeration of the application's views.
///
/// Holds the actual view instances that implement the rendering logic.
enum View {
    /// Empty state when no view is active.
    Empty,
    /// The overview view instance.
    Overview(StorageView),
    /// The acquisition view instance.
    Acquisition(AcquisitionView),
}

impl ViewApi for View {
    /// Renders the current view.
    ///
    /// # Arguments
    /// * `publish` - Function to publish `UiInputEvent`s.
    /// * `ctx` - Egui context for rendering.
    ///
    /// # Returns
    /// `Result<(), String>` indicating success or an error message.
    fn render<F: Fn(AppEvent) + ?Sized>(
        &mut self,
        publish: &F,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        match self {
            Self::Overview(v) => v.render(publish, ctx),
            Self::Acquisition(v) => v.render(publish, ctx),
            Self::Empty => Ok(()),
        }
    }
}

impl From<ViewState> for View {
    /// Converts a `ViewState` into a `View`.
    ///
    /// Initializes the appropriate view based on the state.
    ///
    /// # Arguments
    /// * `val` - The `ViewState` to convert.
    ///
    /// # Returns
    /// A `View` instance.
    fn from(val: ViewState) -> Self {
        match val {
            ViewState::Acquisition((model, bt_model)) => {
                View::Acquisition(AcquisitionView::new(model, bt_model))
            }
            ViewState::Overview((model, measurement)) => {
                View::Overview(StorageView::new(model, measurement))
            }
        }
    }
}

/// Manages the application's views and handles view updates.
///
/// The `ViewManager` listens for view state changes and updates the active view accordingly.
pub struct ViewManager {
    /// Sender for application events.
    e_tx: Sender<AppEvent>,
    /// The currently active view wrapped in a thread-safe `Arc<RwLock>`.
    active_view: Arc<RwLock<View>>,
    /// Handle for the background task that listens for view state changes.
    _task_handle: JoinHandle<()>,
}

impl ViewManager {
    /// Creates a new `ViewManager`.
    ///
    /// # Arguments
    /// * `v_rx` - Receiver for `ViewState` updates.
    /// * `e_tx` - Sender for `AppEvent`s.
    ///
    /// # Returns
    /// A new instance of `ViewManager`.
    pub fn new(mut v_rx: Receiver<ViewState>, e_tx: Sender<AppEvent>) -> Self {
        let active_view = Arc::new(RwLock::new(View::Empty));
        let task_view = active_view.clone();
        let _task_handle = tokio::spawn(async move {
            while let Ok(s) = v_rx.recv().await {
                *task_view.write().await = s.into();
            }
        });

        Self {
            e_tx,
            active_view,
            _task_handle,
        }
    }

    /// Publishes a `UiInputEvent` to the application event stream.
    ///
    /// # Arguments
    /// * `event` - The `UiInputEvent` to publish.
    fn publish(&self, event: AppEvent) {
        if let Err(e) = self.e_tx.send(event) {
            error!("View failed to send event: {}", e.to_string())
        }
    }
}

impl App for ViewManager {
    /// Updates the application's UI.
    ///
    /// Called by the eframe framework to render the UI each frame.
    ///
    /// # Arguments
    /// * `ctx` - The Egui context.
    /// * `_frame` - The eframe frame (unused).
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Set the UI scaling factor for better readability.
        ctx.set_pixels_per_point(1.5);
        if let Err(e) = self
            .active_view
            .blocking_write()
            .render(&|e| self.publish(e), ctx)
        {
            error!("View failed to render: {}", e)
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::{application::tests::{MockBluetooth}, measurement::MeasurementData};

    fn setup_test_manager() -> (ViewManager, Sender<ViewState>) {
        let (v_tx, v_rx) = tokio::sync::broadcast::channel(1);
        let (e_tx, _e_rx) = tokio::sync::broadcast::channel(1);
        let manager = ViewManager::new(v_rx, e_tx);
        (manager, v_tx)
    }

    #[tokio::test]
    async fn test_view_manager_initial_state() {
        let (manager, _v_tx) = setup_test_manager();
        let view = manager.active_view.read().await;
        assert!(matches!(&*view, View::Empty));
    }

    #[tokio::test]
    async fn test_view_manager_state_switch() {
        let (manager, v_tx) = setup_test_manager();
        v_tx.send(ViewState::Acquisition((
            Arc::new(RwLock::new(MeasurementData::default())) as ModelHandle<dyn MeasurementModelApi>,
            Arc::new(RwLock::new(MockBluetooth::new())) as ModelHandle<dyn BluetoothModelApi>,
        )))
        .unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        let view = manager.active_view.read().await;
        assert!(matches!(&*view, View::Acquisition(_)));
    }

}