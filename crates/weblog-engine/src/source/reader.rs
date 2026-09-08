//! 줄 단위 스트리밍 리더. 파일 전체를 메모리에 올리지 않고 줄 길이에 상한을 둔다.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use flate2::bufread::MultiGzDecoder;
use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};

/// 파일 읽기 버퍼 크기. HDD 순차 읽기를 고려해 크게 잡는다.
const READ_BUF_BYTES: usize = 1024 * 1024;
/// 내용 검증에 쓰는 선두 바이트 수.
const HEAD_CHECK_BYTES: usize = 64 * 1024;

/// 압축 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Compression {
    /// 압축 없음.
    None,
    /// gzip(연결된 멤버 포함).
    Gzip,
}

impl Compression {
    /// 저장용 문자열.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Gzip => "gzip",
        }
    }

    /// 선두 바이트로 판별한다. 파일명은 후보 선정에만 쓰고 내용으로 확인한다.
    pub fn detect(path: &Path) -> EngineResult<Self> {
        let mut f = File::open(path)?;
        let mut magic = [0u8; 2];
        let n = f.read(&mut magic)?;
        Ok(if n == 2 && magic == [0x1f, 0x8b] {
            Self::Gzip
        } else {
            Self::None
        })
    }
}

/// 파일 식별 정보. 크기·수정 시각은 빠른 후보 판정용이고 해시는 내용 검증용이다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceIdentity {
    /// 파일 크기(압축된 그대로).
    pub file_size: u64,
    /// 수정 시각(Unix 초). 얻을 수 없으면 `None`.
    pub modified_unix: Option<i64>,
    /// 압축 방식.
    pub compression: Compression,
    /// 선두 바이트의 SHA-256(16진수).
    pub head_hash: String,
    /// 해시에 사용한 바이트 수.
    pub head_bytes: u64,
    /// 전체 파일 SHA-256. 스트리밍으로 계산하며 비용이 크므로 요청 시에만 채운다.
    pub full_hash: Option<String>,
}

/// 파일 크기·수정 시각 스냅샷. 가져오기 도중 변경 감지에 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatSnapshot {
    /// 파일 크기.
    pub file_size: u64,
    /// 수정 시각(Unix 초).
    pub modified_unix: Option<i64>,
}

impl StatSnapshot {
    /// 메타데이터를 읽는다.
    pub fn read(path: &Path) -> EngineResult<Self> {
        let meta = std::fs::metadata(path)?;
        Ok(Self {
            file_size: meta.len(),
            modified_unix: modified_unix(&meta),
        })
    }
}

fn modified_unix(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_secs()).ok())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::Digest;
    let digest = sha2::Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

impl SourceIdentity {
    /// 파일에서 빠른 식별 정보(크기·시각·선두 해시)를 읽는다.
    pub fn read(path: &Path) -> EngineResult<Self> {
        let meta = std::fs::metadata(path)?;
        let compression = Compression::detect(path)?;
        let mut f = File::open(path)?;
        let mut head = vec![0u8; HEAD_CHECK_BYTES];
        let mut filled = 0;
        while filled < head.len() {
            let n = f.read(&mut head[filled..])?;
            if n == 0 {
                break;
            }
            filled += n;
        }
        head.truncate(filled);
        Ok(Self {
            file_size: meta.len(),
            modified_unix: modified_unix(&meta),
            compression,
            head_hash: sha256_hex(&head),
            head_bytes: filled as u64,
            full_hash: None,
        })
    }

    /// 전체 파일 해시까지 계산한다. 파일 전체를 순차로 읽으며 메모리는 버퍼 크기만 쓴다.
    pub fn read_full(path: &Path) -> EngineResult<Self> {
        let mut id = Self::read(path)?;
        id.full_hash = Some(Self::compute_full_hash(path)?);
        Ok(id)
    }

    /// 전체 파일 SHA-256(16진수).
    pub fn compute_full_hash(path: &Path) -> EngineResult<String> {
        use sha2::Digest;
        let mut f = BufReader::with_capacity(READ_BUF_BYTES, File::open(path)?);
        let mut hasher = sha2::Sha256::new();
        loop {
            let chunk = f.fill_buf()?;
            if chunk.is_empty() {
                break;
            }
            hasher.update(chunk);
            let n = chunk.len();
            f.consume(n);
        }
        Ok(hasher
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect())
    }

    /// 빠른 검증: 크기와 선두 해시가 같은지. 수정 시각은 복사·이동으로 바뀔 수 있어 비교하지 않는다.
    pub fn matches(&self, other: &Self) -> bool {
        self.file_size == other.file_size
            && self.head_hash == other.head_hash
            && self.head_bytes == other.head_bytes
    }

    /// 불일치 항목 설명. 같으면 `None`.
    pub fn mismatch_reason(&self, other: &Self) -> Option<String> {
        if self.file_size != other.file_size {
            return Some(format!("크기 {} → {}", self.file_size, other.file_size));
        }
        if self.head_hash != other.head_hash || self.head_bytes != other.head_bytes {
            return Some("선두 내용 해시 불일치".to_owned());
        }
        if let (Some(a), Some(b)) = (&self.full_hash, &other.full_hash) {
            if a != b {
                return Some("전체 내용 해시 불일치".to_owned());
            }
        }
        None
    }
}

/// 한 줄의 내용.
#[derive(Debug, PartialEq, Eq)]
pub enum LineContent<'a> {
    /// 유효한 UTF-8 텍스트(개행·CR 제거).
    Text(&'a str),
    /// UTF-8이 아닌 바이트 포함. 손실 변환하지 않는다.
    InvalidUtf8,
    /// 줄 길이가 상한을 넘어 내용을 버림.
    TooLong,
}

/// 줄 하나. 오프셋은 논리 스트림(압축 해제 후) 기준이다.
#[derive(Debug, PartialEq, Eq)]
pub struct RawLine<'a> {
    /// 논리 줄 번호(1부터).
    pub line_number: u64,
    /// 줄 시작 오프셋.
    pub start_offset: u64,
    /// 다음 줄 시작 오프셋(체크포인트 값).
    pub next_offset: u64,
    /// 내용.
    pub content: LineContent<'a>,
}

enum Inner {
    Plain(BufReader<File>),
    Gzip(BufReader<MultiGzDecoder<BufReader<File>>>),
}

impl Inner {
    fn as_bufread(&mut self) -> &mut dyn BufRead {
        match self {
            Self::Plain(r) => r,
            Self::Gzip(r) => r,
        }
    }
}

/// 줄 단위 리더.
pub struct LineReader {
    inner: Inner,
    max_line_bytes: usize,
    buf: Vec<u8>,
    offset: u64,
    line_number: u64,
    first_line: bool,
}

impl std::fmt::Debug for LineReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LineReader")
            .field("offset", &self.offset)
            .field("line_number", &self.line_number)
            .finish_non_exhaustive()
    }
}

impl LineReader {
    /// 파일을 연다. 압축은 내용으로 판별한다.
    pub fn open(path: &Path, max_line_bytes: usize) -> EngineResult<Self> {
        let compression = Compression::detect(path)?;
        Self::open_with(path, compression, max_line_bytes)
    }

    /// 압축 방식을 지정해 연다.
    pub fn open_with(
        path: &Path,
        compression: Compression,
        max_line_bytes: usize,
    ) -> EngineResult<Self> {
        if max_line_bytes == 0 {
            return Err(EngineError::Limit(
                "줄 길이 상한은 0보다 커야 함".to_owned(),
            ));
        }
        let file = File::open(path)?;
        let inner = match compression {
            Compression::None => Inner::Plain(BufReader::with_capacity(READ_BUF_BYTES, file)),
            Compression::Gzip => Inner::Gzip(BufReader::with_capacity(
                READ_BUF_BYTES,
                MultiGzDecoder::new(BufReader::with_capacity(READ_BUF_BYTES, file)),
            )),
        };
        Ok(Self {
            inner,
            max_line_bytes,
            buf: Vec::with_capacity(4096),
            offset: 0,
            line_number: 0,
            first_line: true,
        })
    }

    /// 현재 논리 오프셋.
    pub fn offset(&self) -> u64 {
        self.offset
    }

    /// 지금까지 읽은 줄 수.
    pub fn line_number(&self) -> u64 {
        self.line_number
    }

    /// 체크포인트 위치로 이동한다. 일반 파일은 seek, gzip은 재생하며 건너뛴다.
    /// `line_number`는 그 위치의 직전까지 읽은 줄 수다.
    pub fn resume_at(&mut self, offset: u64, line_number: u64) -> EngineResult<()> {
        if offset == 0 {
            return Ok(());
        }
        match &mut self.inner {
            Inner::Plain(r) => {
                r.seek(SeekFrom::Start(offset))?;
            }
            Inner::Gzip(r) => {
                let mut remaining = offset;
                while remaining > 0 {
                    let available = r.fill_buf()?;
                    if available.is_empty() {
                        return Err(EngineError::Io(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "재개 위치가 압축 해제 스트림 길이를 넘음",
                        )));
                    }
                    let take = usize::try_from(remaining.min(available.len() as u64))
                        .map_err(|_| EngineError::Limit("오프셋 변환 실패".to_owned()))?;
                    r.consume(take);
                    remaining -= take as u64;
                }
            }
        }
        self.offset = offset;
        self.line_number = line_number;
        self.first_line = false;
        Ok(())
    }

    /// 다음 줄을 읽는다. 끝이면 `None`.
    pub fn next_line(&mut self) -> EngineResult<Option<RawLine<'_>>> {
        self.buf.clear();
        let mut too_long = false;
        let mut consumed_total: u64 = 0;
        let reader = self.inner.as_bufread();
        loop {
            let available = reader.fill_buf()?;
            if available.is_empty() {
                if consumed_total == 0 {
                    return Ok(None);
                }
                break;
            }
            let (chunk, found_newline) = match available.iter().position(|&b| b == b'\n') {
                Some(i) => (&available[..=i], true),
                None => (available, false),
            };
            let chunk_len = chunk.len();
            if !too_long {
                let payload = if found_newline {
                    &chunk[..chunk_len - 1]
                } else {
                    chunk
                };
                if self.buf.len() + payload.len() > self.max_line_bytes {
                    too_long = true;
                    self.buf.clear();
                } else {
                    self.buf.extend_from_slice(payload);
                }
            }
            reader.consume(chunk_len);
            consumed_total += chunk_len as u64;
            if found_newline {
                break;
            }
        }
        let start_offset = self.offset;
        self.offset += consumed_total;
        self.line_number += 1;
        let content = if too_long {
            LineContent::TooLong
        } else {
            if self.buf.last() == Some(&b'\r') {
                self.buf.pop();
            }
            let mut bytes: &[u8] = &self.buf;
            if self.first_line {
                bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF][..]).unwrap_or(bytes);
            }
            match std::str::from_utf8(bytes) {
                Ok(s) => LineContent::Text(s),
                Err(_) => LineContent::InvalidUtf8,
            }
        };
        self.first_line = false;
        Ok(Some(RawLine {
            line_number: self.line_number,
            start_offset,
            next_offset: self.offset,
            content,
        }))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::io::Write;

    fn write_temp(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(bytes).unwrap();
        f.flush().unwrap();
        f
    }

    fn gzip_temp(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        enc.write_all(bytes).unwrap();
        write_temp(&enc.finish().unwrap())
    }

    fn collect(reader: &mut LineReader) -> Vec<(u64, u64, u64, String)> {
        let mut out = Vec::new();
        while let Some(line) = reader.next_line().unwrap() {
            let text = match line.content {
                LineContent::Text(s) => s.to_owned(),
                LineContent::InvalidUtf8 => "<invalid>".to_owned(),
                LineContent::TooLong => "<toolong>".to_owned(),
            };
            out.push((line.line_number, line.start_offset, line.next_offset, text));
        }
        out
    }

    #[test]
    fn reads_lines_with_offsets_and_strips_crlf() {
        let f = write_temp(b"ab\r\ncd\n\nef");
        let mut r = LineReader::open(f.path(), 1024).unwrap();
        assert_eq!(
            collect(&mut r),
            vec![
                (1, 0, 4, "ab".to_owned()),
                (2, 4, 7, "cd".to_owned()),
                (3, 7, 8, String::new()),
                (4, 8, 10, "ef".to_owned()),
            ]
        );
    }

    #[test]
    fn bom_is_stripped_only_on_first_line_and_offsets_count_bom_bytes() {
        let f = write_temp(b"\xEF\xBB\xBFx\ny\n");
        let mut r = LineReader::open(f.path(), 1024).unwrap();
        let lines = collect(&mut r);
        assert_eq!(lines[0], (1, 0, 5, "x".to_owned()));
        assert_eq!(lines[1], (2, 5, 7, "y".to_owned()));
    }

    #[test]
    fn invalid_utf8_is_reported_not_lossy_converted() {
        let f = write_temp(b"ok\n\xFF\xFE bad\nok2\n");
        let mut r = LineReader::open(f.path(), 1024).unwrap();
        let lines = collect(&mut r);
        assert_eq!(lines[1].3, "<invalid>");
        assert_eq!(lines[2].3, "ok2");
    }

    #[test]
    fn too_long_line_is_skipped_without_buffering_and_offsets_stay_correct() {
        let long = vec![b'a'; 5000];
        let mut data = b"short\n".to_vec();
        data.extend_from_slice(&long);
        data.extend_from_slice(b"\nafter\n");
        let f = write_temp(&data);
        let mut r = LineReader::open(f.path(), 100).unwrap();
        let lines = collect(&mut r);
        assert_eq!(lines[1].3, "<toolong>");
        assert_eq!(lines[1].2, 6 + 5001);
        assert_eq!(lines[2], (3, 6 + 5001, 6 + 5001 + 6, "after".to_owned()));
    }

    #[test]
    fn gzip_is_detected_by_magic_bytes_and_read_as_logical_stream() {
        let f = gzip_temp(b"l1\nl2\nl3\n");
        assert_eq!(Compression::detect(f.path()).unwrap(), Compression::Gzip);
        let mut r = LineReader::open(f.path(), 1024).unwrap();
        let lines = collect(&mut r);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[2], (3, 6, 9, "l3".to_owned()));
    }

    #[test]
    fn concatenated_gzip_members_are_read_continuously() {
        let mut a = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        a.write_all(b"m1\n").unwrap();
        let mut bytes = a.finish().unwrap();
        let mut b = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
        b.write_all(b"m2\n").unwrap();
        bytes.extend(b.finish().unwrap());
        let f = write_temp(&bytes);
        let mut r = LineReader::open(f.path(), 1024).unwrap();
        assert_eq!(collect(&mut r).len(), 2);
    }

    #[test]
    fn resume_at_offset_continues_from_checkpoint_for_plain_and_gzip() {
        for gz in [false, true] {
            let data = b"l1\nl2\nl3\n";
            let f = if gz {
                gzip_temp(data)
            } else {
                write_temp(data)
            };
            let mut r = LineReader::open(f.path(), 1024).unwrap();
            r.resume_at(3, 1).unwrap();
            let lines = collect(&mut r);
            assert_eq!(
                lines,
                vec![(2, 3, 6, "l2".to_owned()), (3, 6, 9, "l3".to_owned())],
                "gzip={gz}"
            );
        }
    }

    #[test]
    fn source_identity_matches_same_content_and_differs_on_change() {
        let a = write_temp(b"same content\n");
        let b = write_temp(b"same content\n");
        let c = write_temp(b"other content\n");
        let ia = SourceIdentity::read(a.path()).unwrap();
        assert!(ia.matches(&SourceIdentity::read(b.path()).unwrap()));
        assert!(!ia.matches(&SourceIdentity::read(c.path()).unwrap()));
    }
}

#[cfg(test)]
mod identity_tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::io::Write;

    #[test]
    fn full_hash_detects_change_beyond_head_window() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.log");
        let mut body = vec![b'x'; HEAD_CHECK_BYTES + 10];
        std::fs::write(&p, &body).unwrap();
        let a = SourceIdentity::read_full(&p).unwrap();
        body[HEAD_CHECK_BYTES + 5] = b'y';
        std::fs::write(&p, &body).unwrap();
        let b = SourceIdentity::read_full(&p).unwrap();
        assert!(
            a.matches(&b),
            "fast check cannot see a change past the head window"
        );
        assert_eq!(
            a.mismatch_reason(&b).as_deref(),
            Some("전체 내용 해시 불일치")
        );
    }

    #[test]
    fn stat_snapshot_changes_when_file_is_appended() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.log");
        std::fs::write(&p, b"one\n").unwrap();
        let before = StatSnapshot::read(&p).unwrap();
        let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(b"two\n").unwrap();
        let after = StatSnapshot::read(&p).unwrap();
        assert_ne!(before.file_size, after.file_size);
    }
}
