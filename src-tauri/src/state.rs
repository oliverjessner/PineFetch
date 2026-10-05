use crate::config::ConfigState;
use crate::database::Database;
use crate::models::AppConfig;
use crate::models::LinkDumpServerStatus;
use crate::process::ProcessState;
use crate::queue::QueueState;
use rusqlite::Connection;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread::JoinHandle;

pub(crate) struct AppState {
    pub(crate) config: ConfigState,
    pub(crate) db: Arc<Database>,
    pub(crate) queue: QueueState,
    pub(crate) processes: ProcessState,
    pub(crate) link_dump_server: Mutex<LinkDumpServerRuntime>,
}
impl AppState {
    pub(crate) fn new(config: AppConfig, connection: Connection) -> Self {
        let db = Arc::new(Database::new(connection));
        Self {
            config: ConfigState::new(config, Arc::clone(&db)),
            db,
            queue: QueueState::default(),
            processes: ProcessState::default(),
            link_dump_server: Mutex::new(LinkDumpServerRuntime::default()),
        }
    }
}
#[derive(Debug, Default)]
pub(crate) struct LinkDumpServerRuntime {
    pub(crate) status: LinkDumpServerStatus,
    pub(crate) shutdown: Option<Arc<AtomicBool>>,
    pub(crate) handle: Option<JoinHandle<()>>,
}
