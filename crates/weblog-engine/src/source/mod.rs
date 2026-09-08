//! 입력 파일 접근. 스트리밍 압축 해제, 줄 단위 읽기, 바이트 상한, 위치 추적.

pub mod reader;

pub mod scan;

pub use reader::{Compression, LineContent, LineReader, RawLine, SourceIdentity, StatSnapshot};
pub use scan::{scan_directory, ScanEntry, ScanError, ScanOptions, ScanResult};
