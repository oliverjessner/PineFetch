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
    pub(crate) ocr: Arc<crate::ocr::OcrState>,
    pub(crate) ocr_preview: crate::ocr_preview::OcrPreviewState,
    pub(crate) history_media: crate::history_media::HistoryMediaState,
    pub(crate) accounts: crate::accounts::AccountState,
    pub(crate) social_insights: crate::social_insights::SocialInsightsState,
    pub(crate) link_dump_server: Mutex<LinkDumpServerRuntime>,
}
impl AppState {
    #[cfg(test)]
    pub(crate) fn new(config: AppConfig, connection: Connection) -> Self {
        let db = Arc::new(Database::new(connection));
        Self::assemble(config, db, QueueState::default())
    }
    pub(crate) fn restore(config: AppConfig, connection: Connection) -> Result<Self, String> {
        let db = Arc::new(Database::new(connection));
        let queue = QueueState::restore(Arc::clone(&db))?;
        Ok(Self::assemble(config, db, queue))
    }
    fn assemble(config: AppConfig, db: Arc<Database>, queue: QueueState) -> Self {
        Self {
            config: ConfigState::new(config, Arc::clone(&db)),
            db,
            queue,
            processes: ProcessState::default(),
            ocr: Arc::new(crate::ocr::OcrState::default()),
            ocr_preview: crate::ocr_preview::OcrPreviewState::default(),
            history_media: crate::history_media::HistoryMediaState::default(),
            accounts: crate::accounts::AccountState::default(),
            social_insights: crate::social_insights::SocialInsightsState::default(),
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
