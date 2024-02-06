use async_trait::async_trait;
use loco_rs::{
    app::{AppContext, Hooks},
    boot::{create_app, BootResult, StartMode},
    controller::{channels::AppChannels, AppRoutes},
    environment::Environment,
    task::Tasks,
    worker::Processor,
    Result,
};

use crate::channels;

pub struct App;
#[async_trait]
impl Hooks for App {
    fn app_name() -> &'static str {
        env!("CARGO_CRATE_NAME")
    }

    fn app_version() -> String {
        format!(
            "{} ({})",
            env!("CARGO_PKG_VERSION"),
            option_env!("BUILD_SHA")
                .or(option_env!("GITHUB_SHA"))
                .unwrap_or("dev")
        )
    }

    async fn boot(mode: StartMode, environment: &Environment) -> Result<BootResult> {
        create_app::<Self>(mode, environment).await
    }

    fn routes(ctx: &AppContext) -> AppRoutes {
        AppRoutes::empty()
            .prefix("/api")
            .add_app_channels(Self::register_channels(ctx))
    }

    fn register_channels(_ctx: &AppContext) -> AppChannels {
        let messages = channels::state::MessageStore::default();

        let channels: AppChannels = AppChannels::builder().with_state(messages).into();
        channels.register.ns("/", channels::application::on_connect);
        channels
    }

    fn connect_workers<'a>(_p: &'a mut Processor, _ctx: &'a AppContext) {}

    fn register_tasks(_tasks: &mut Tasks) {}
}
