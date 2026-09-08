//! 웹 로그 파서·저장·조회 엔진. Tauri에 의존하지 않으며 GUI 없이 테스트할 수 있다.
//!
//! 의존 방향: 애플리케이션 → [`importer`] → [`parse`]/[`source`]/[`store`].
//! 원문은 저장하지 않는다. 파싱 필드와 출처 파일·줄 위치만 저장한다.

#![deny(missing_docs)]

pub mod detect;
pub mod error;
pub mod export;
pub mod format;
pub mod importer;
pub mod parse;
pub mod preview;
pub mod reconstruct;
pub mod source;
pub mod store;

pub use error::{EngineError, EngineResult};
