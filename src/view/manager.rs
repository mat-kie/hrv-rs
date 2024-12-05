use std::sync::Arc;

use eframe::App;
use log::error;
use tokio::{sync::{
    broadcast::{Receiver, Sender},
    RwLock,
}, task::JoinHandle};

use crate::{
    core::{events::{AppEvent, UiInputEvent}, view_trait::ViewApi},
    model::{
        acquisition::{AcquisitionModel, AcquisitionModelApi},
        bluetooth::BluetoothModelApi,
        storage::{ModelHandle, StorageModel},
    },
};

use super::{ acquisition::AcquisitionView, overview::StorageView};

#[derive(Clone)]
pub enum ViewState {
    Overview(ModelHandle<StorageModel<AcquisitionModel>>),
    Acquisition((ModelHandle<dyn AcquisitionModelApi>, ModelHandle<dyn BluetoothModelApi>)),
}

enum View {
    NoView,
    Overview(StorageView<StorageModel<AcquisitionModel>>),
    Acquisition(AcquisitionView),
}

impl ViewApi for View {
    fn render<F: Fn(UiInputEvent) + ?Sized>(
        &mut self,
        publish: &F,
        ctx: &egui::Context,
    ) -> Result<(), String> {
        match self {
            Self::Overview(v) => v.render(publish, ctx),
            Self::Acquisition(v) => v.render(publish, ctx),
            Self::NoView=>{Ok(())}
        }
    }
}

impl From<ViewState> for View {
    fn from(val: ViewState) -> Self {
        match val {
            ViewState::Acquisition((model, bt_model)) => View::Acquisition(AcquisitionView::new(model, bt_model)),
            ViewState::Overview(model) => {
                View::Overview(StorageView::<StorageModel<AcquisitionModel>>::new(model))
            },
        }
    }
}
pub struct ViewManager {
    e_tx: Sender<AppEvent>,
    active_view: Arc<RwLock<View>>,
    _task_handle: JoinHandle<()>
}

impl ViewManager {
    pub fn new(mut v_rx: Receiver<ViewState>, e_tx: Sender<AppEvent>) -> Self {
      let active_view = Arc::new(RwLock::new(View::NoView));
      let task_view = active_view.clone();
      let _task_handle = 
      tokio::spawn(async move{
        while let Ok(s) = v_rx.recv().await{
          *task_view.write().await = s.into();
        }
      });
        
        Self {
            e_tx,
            active_view,
            _task_handle
        }
    }

    fn publish(&self, event: UiInputEvent) {
        if let Err(e)  =self.e_tx.send(AppEvent::UiInput(event)){
            error!("View failed to send event:{}", e.to_string())
        }
    }
}

impl App for ViewManager {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        ctx.set_pixels_per_point(1.5);
        if let Err(e) = 
        self.active_view
            .blocking_write()
            .render(&|e| self.publish(e), ctx){
                error!("view failed to render: {}", e)
            }
    }
}
