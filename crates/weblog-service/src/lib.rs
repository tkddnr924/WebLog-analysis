//! 애플리케이션 서비스 계층. UI(Tauri 명령)와 엔진 사이에서 프로젝트 상태, 백그라운드 가져오기,
//! 취소, 진행 이벤트 빈도 제한, 읽기 연결 풀과 무거운 조회 직렬화를 담당한다. Tauri에 의존하지 않는다.

#![deny(missing_docs)]

pub mod dto;
mod project;
mod service;

pub use dto::*;
pub use service::{Service, ServiceConfig, ServiceError, ServiceEvent, ServiceResult};
