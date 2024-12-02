//! Application Controller
//!
//! This module defines the main controller responsible for orchestrating the application.
//! It initializes and manages other controllers and coordinates the overall application flow.

use super::{acquisition::DataAcquisitionApi, bluetooth::handle_event};
use crate::{
    controller::bluetooth::BluetoothApi,
    core::{events::{AppEvent, BluetoothEvent}, view_trait::ViewApi},
    model::{
        acquisition::AcquisitionModelApi,
        bluetooth::{AdapterHandle, BluetoothModelApi},
    },
    view::{bluetooth::BluetoothView, hrv_analysis::HrvView, manager::ViewManager},
};

use log::{error, info};
use std::{
    marker::PhantomData,
    sync::{Arc, Mutex},
};
use tokio::sync::mpsc::{Receiver};
use tokio::sync::mpsc::Sender;

/// Main application controller.
///
/// This structure manages the lifecycle of other controllers and handles application-level events.
pub struct AppController<
    AHT: AdapterHandle + Send + 'static,
    ACT: DataAcquisitionApi + Send + 'static,
    BTCT: BluetoothApi<AHT> + Send + 'static,
    BTMT: BluetoothModelApi<AHT> + Send + 'static,
    //ACMT: AcquisitionModelApi + Send + 'static,
> {
    /// Bluetooth model.
    bt_model: Arc<tokio::sync::Mutex<BTMT>>,
    /// Acquisition model.
    acq_model: Arc<Mutex<dyn AcquisitionModelApi>>,
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
    > AppController<AHT, ACT, BTCT, BTMT>
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
        acq_model: Arc<Mutex<dyn AcquisitionModelApi>>,
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
        let (view_tx, view_rx) = tokio::sync::mpsc::channel::<Box<dyn ViewApi>>(16);
        let (event_tx, event_rx) = tokio::sync::mpsc::channel(16);
        self.ble_controller.initialize(event_tx.clone());
        if let Err(e) = view_tx
            .blocking_send(Box::new(BluetoothView::new(self.bt_model.clone(), event_tx.clone()))){
                error!("couold not send initial view to view manager: {}", e);
            }

        std::mem::drop(tokio::spawn(self.event_handler(view_tx, event_rx, event_tx.clone(), gui_ctx)));
        let _ = event_tx.try_send(AppEvent::Bluetooth(BluetoothEvent::DiscoverAdapters));
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
        view_ch: Sender<Box<dyn ViewApi>>,
        mut event_ch_rx: Receiver<AppEvent>,
        event_ch_tx: Sender<AppEvent>,
        gui_ctx: egui::Context,
    ) {
        while let Some(evt) = event_ch_rx.recv().await {
            match evt {
                AppEvent::Bluetooth(btev) => {
                    if let Err(e) = handle_event(&mut self.ble_controller, btev).await {
                        error!("Bluetooth event error: {:?}", e);
                    }

                    if let Err(e) = if self.bt_model.lock().await.is_listening_to().is_some() {
                        view_ch.send(Box::new(HrvView::new(self.acq_model.clone(), event_ch_tx.clone()))).await
                    } else {
                        view_ch.send(Box::new(BluetoothView::new(self.bt_model.clone(),  event_ch_tx.clone()))).await
                        
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
                AppEvent::AcquisitionStopReq(path) => {
                    if let Err(e) = self.acq_controller.store_acquisition(path) {
                        error!("Failed to store acquisition: {}", e);
                    }
                }
                AppEvent::SelectModel(model)=>{
                    let self_lck = self.acq_model.lock().unwrap();
                    let evt_lck = model.lock().unwrap();
                }
            }
            gui_ctx.request_repaint();
        }
    }
}
