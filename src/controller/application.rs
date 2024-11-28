//! Application Controller
//!
//! This module defines the main controller responsible for orchestrating the application.
//! It initializes and manages other controllers and coordinates the overall application flow.

use super::{acquisition::DataAcquisitionApi, bluetooth::handle_event};
use crate::{
    controller::bluetooth::BluetoothApi,
    core::events::{AppEvent, BluetoothEvent, ViewState},
    model::{
        acquisition::AcquisitionModelApi,
        bluetooth::{AdapterHandle, BluetoothModelApi},
    },
    view::manager::ViewManager,
};

use log::{error, info, warn};
use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use tokio::sync::broadcast::{Receiver, Sender};

/// Main application controller.
///
/// This structure manages the lifecycle of other controllers and handles application-level events.
pub struct AppController<
    AHT: AdapterHandle + Send + 'static,
    ACT: DataAcquisitionApi + Send + 'static,
    BTCT: BluetoothApi<AHT> + Send + 'static,
    BTMT: BluetoothModelApi<AHT> + Send + 'static,
    ACMT: AcquisitionModelApi + Send + 'static,
> {
    /// Bluetooth model.
    bt_model: Arc<tokio::sync::Mutex<BTMT>>,
    /// Acquisition model.
    acq_model: Arc<Mutex<ACMT>>,
    /// Bluetooth controller.
    ble_controller: BTCT,
    /// Data acquisition controller.
    acq_controller: ACT,
    /// Marker for type parameter `AHT`.
    _marker: PhantomData<AHT>,
}

impl<
        AHT: AdapterHandle + Send,
        ACT: DataAcquisitionApi + Send,
        BTCT: BluetoothApi<AHT> + Send,
        BTMT: BluetoothModelApi<AHT> + Send,
        ACMT: AcquisitionModelApi + Send,
    > AppController<AHT, ACT, BTCT, BTMT, ACMT>
{
    /// Creates a new `AppController`.
    ///
    /// # Arguments
    /// - `bt_model`: The Bluetooth model.
    /// - `acq_model`: The acquisition model.
    /// - `ble_controller`: The Bluetooth controller.
    /// - `acq_controller`: The acquisition controller.
    ///
    /// # Returns
    /// A new `AppController` instance.
    pub fn new(
        bt_model: Arc<tokio::sync::Mutex<BTMT>>,
        acq_model: Arc<Mutex<ACMT>>,
        ble_controller: BTCT,
        acq_controller: ACT,
    ) -> Self {
        info!("Initializing AppController.");
        Self {
            bt_model,
            acq_model,
            ble_controller,
            acq_controller,
            _marker: Default::default(),
        }
    }

    /// Launches the application.
    ///
    /// This initializes the UI context, starts event handling, and sets up the main view.
    ///
    /// # Arguments
    /// - `gui_ctx`: The GUI context.
    ///
    /// # Returns
    /// The view manager to coordinate application views.
    pub fn launch(mut self, gui_ctx: egui::Context) -> ViewManager<AHT> {
        let (view_tx, view_rx) = tokio::sync::broadcast::channel(16);
        let (event_tx, event_rx) = tokio::sync::broadcast::channel(16);
        self.ble_controller.initialize(event_tx.clone());
        view_tx
            .send(ViewState::BluetoothSelectorView(self.bt_model.clone()))
            .unwrap_or_else(|e| {
                warn!("Failed to send initial ViewState: {}", e);
                0
            });

        std::mem::drop(tokio::spawn(self.event_handler(view_tx, event_rx, gui_ctx)));
        let _ = event_tx.send(AppEvent::Bluetooth(BluetoothEvent::DiscoverAdapters));
        ViewManager::new(view_rx, event_tx)
    }

    /// Asynchronous event handler.
    ///
    /// Processes application-level events and delegates them to appropriate controllers.
    ///
    /// # Arguments
    /// - `view_ch`: Sender for view updates.
    /// - `event_ch`: Receiver for application events.
    /// - `gui_ctx`: The GUI context.
    async fn event_handler(
        mut self,
        view_ch: Sender<ViewState<AHT>>,
        mut event_ch: Receiver<AppEvent>,
        gui_ctx: egui::Context,
    ) {
        while let Ok(evt) = event_ch.recv().await {
            match evt {
                AppEvent::Bluetooth(btev) => {
                    if let Err(e) = handle_event(&mut self.ble_controller, btev).await {
                        error!("Bluetooth event error: {:?}", e);
                    }

                    if let Err(e) = if self.bt_model.lock().await.is_listening_to().is_some() {
                        view_ch.send(ViewState::AcquisitionView(self.acq_model.clone()))
                    } else {
                        view_ch.send(ViewState::BluetoothSelectorView(self.bt_model.clone()))
                    } {
                        error!("Failed to send ViewState update: {}", e);
                    }
                }
                AppEvent::Data(hrev) => {
                    if let Err(e) = self.acq_controller.handle_event(hrev) {
                        error!("Failed to handle HRV event: {}", e);
                    }
                }
                AppEvent::AcquisitionStartReq => {
                    self.acq_controller.new_acquisition();
                }
                AppEvent::AcquisitionStopReq => {
                    if let Err(e) = self.acq_controller.store_acquisition() {
                        error!("Failed to store acquisition: {}", e);
                    }
                }
            }
            gui_ctx.request_repaint();
        }
    }
}
