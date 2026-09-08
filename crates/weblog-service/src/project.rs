//! 열린 프로젝트: 쓰기 저장소 하나와 읽기 연결 풀.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use weblog_engine::store::{Reader, Store, StoreConfig};
use weblog_engine::EngineResult;

/// 읽기 연결 풀. 무거운 조회는 별도 잠금으로 동시 1개로 제한한다.
pub(crate) struct ReaderPool {
    readers: Vec<Mutex<Reader>>,
    heavy: Mutex<()>,
}

impl ReaderPool {
    pub(crate) fn new(store: &Store, size: usize) -> EngineResult<Self> {
        let mut readers = Vec::with_capacity(size.max(1));
        for _ in 0..size.max(1) {
            readers.push(Mutex::new(store.open_reader()?));
        }
        Ok(Self {
            readers,
            heavy: Mutex::new(()),
        })
    }

    /// 비어 있는 연결을 잡는다. 모두 사용 중이면 첫 연결을 기다린다.
    pub(crate) fn acquire(&self) -> MutexGuard<'_, Reader> {
        for r in &self.readers {
            if let Ok(g) = r.try_lock() {
                return g;
            }
        }
        self.readers[0].lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 무거운 조회 잠금(동시 1개).
    pub(crate) fn heavy(&self) -> MutexGuard<'_, ()> {
        self.heavy.lock().unwrap_or_else(|e| e.into_inner())
    }
}

pub(crate) struct Project {
    pub(crate) path: PathBuf,
    pub(crate) store: Arc<Mutex<Store>>,
    pub(crate) readers: Arc<ReaderPool>,
    /// 내보내기 전용 연결. 내보내기 스레드가 실행 내내 잡는다.
    pub(crate) export_reader: Mutex<Reader>,
}

impl Project {
    pub(crate) fn open(
        path: PathBuf,
        cfg: &StoreConfig,
        readers: usize,
    ) -> EngineResult<(Self, Vec<i64>)> {
        let store = Store::open(&path, cfg)?;
        let interrupted = store.mark_interrupted_jobs()?;
        let pool = ReaderPool::new(&store, readers)?;
        let export_reader = Mutex::new(store.open_reader()?);
        Ok((
            Self {
                path,
                store: Arc::new(Mutex::new(store)),
                readers: Arc::new(pool),
                export_reader,
            },
            interrupted,
        ))
    }
}
