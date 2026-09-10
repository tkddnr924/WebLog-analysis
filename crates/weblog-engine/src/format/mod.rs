//! 포맷 정의 모델과 프리셋. 퍼즐/YAML 편집기와 파서가 같은 정의를 공유한다.

pub mod compile;
pub mod library;
pub mod model;
pub mod presets;
pub mod validate;
pub mod yaml;

pub use compile::{CompiledBlocks, MatchBuf};
pub use library::{ProfileLibrary, ProfileListing, StoredProfile};
pub use model::{
    Block, Capture, FieldDef, FieldKind, FormatProfile, ServerHint, Strategy, TimestampFormat,
    TimezonePolicy, SCHEMA_VERSION,
};
pub use validate::ValidationIssue;
