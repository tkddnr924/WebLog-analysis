//! 엔진 선행 실험 CLI. 합성 로그 생성, 가져오기, 페이지 조회 지연 측정, 상세·재구성 확인.

mod synth;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand, ValueEnum};

use weblog_engine::detect::{default_candidates, detect_and_group};
use weblog_engine::format::{presets, FormatProfile};
use weblog_engine::importer::resume_import;
use weblog_engine::importer::{run_import, ImportConfig, ImportRequest};
use weblog_engine::preview::{preview_file, PreviewConfig};
use weblog_engine::source::{scan_directory, ScanOptions};
use weblog_engine::store::{LogFilter, LogQuery, PageRequest, SortOrder, Store, StoreConfig};

#[derive(Parser)]
#[command(name = "weblog", about = "WebLog 엔진 선행 실험 CLI", version)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Clone, Copy, ValueEnum)]
enum GenFormat {
    Common,
    Combined,
    W3c,
    CustomPipe,
}

#[derive(Subcommand)]
enum ProfileAction {
    /// 프리셋 이름 또는 JSON/YAML 파일을 검증하고 문제 목록을 출력한다.
    Validate { spec: String },
    /// 프리셋 이름 또는 JSON 파일을 YAML로 출력한다.
    ToYaml { spec: String },
}

#[derive(Subcommand)]
enum Command {
    /// 합성 로그를 생성한다.
    Gen {
        #[arg(long, value_enum, default_value = "combined")]
        format: GenFormat,
        #[arg(long, default_value_t = 100_000)]
        lines: u64,
        #[arg(long, default_value_t = 42)]
        seed: u64,
        #[arg(long, default_value_t = 0.005)]
        error_rate: f64,
        #[arg(long, default_value_t = 0.001)]
        blank_rate: f64,
        #[arg(long, default_value_t = 5000)]
        unique_ips: usize,
        #[arg(long, default_value_t = 2000)]
        unique_paths: usize,
        /// gzip으로 압축해 저장한다.
        #[arg(long)]
        gzip: bool,
        #[arg(long)]
        out: PathBuf,
    },
    /// 파일을 파싱해 DuckDB에 저장한다.
    Import {
        #[arg(long)]
        db: PathBuf,
        /// 프리셋 이름(common, combined, apache_combined, nginx_combined, iis_w3c) 또는 프로필 JSON 경로.
        #[arg(long)]
        format: String,
        #[arg(long, default_value_t = 50_000)]
        batch_rows: usize,
        #[arg(long, default_value_t = 32 * 1024 * 1024)]
        batch_bytes: usize,
        #[arg(long, default_value_t = 64 * 1024)]
        max_line_bytes: usize,
        /// DuckDB memory_limit(예: 4GB). 프로세스 전체 상한이 아니다.
        #[arg(long)]
        memory_limit: Option<String>,
        #[arg(long)]
        threads: Option<u32>,
        #[arg(long)]
        temp_dir: Option<PathBuf>,
        /// 진행 상황을 배치마다 stderr에 출력한다.
        #[arg(long)]
        progress: bool,
        /// 재파싱: 이 작업을 대체하는 새 결과 버전을 만든다(완료 후 `activate` 필요).
        #[arg(long)]
        replaces_job: Option<i64>,
        #[arg(required = true)]
        paths: Vec<PathBuf>,
    },
    /// 중단된(interrupted/cancelled/failed) 작업을 마지막 확정 배치 다음부터 재개한다.
    Resume {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        job: i64,
        #[arg(long, default_value_t = 50_000)]
        batch_rows: usize,
        /// 재개 전 전체 파일 해시를 검증한다(파일 전체를 한 번 더 읽음).
        #[arg(long)]
        full_verify: bool,
        #[arg(long)]
        memory_limit: Option<String>,
        #[arg(long)]
        threads: Option<u32>,
        #[arg(long)]
        progress: bool,
    },
    /// 경로 아래를 재귀 탐색하고 파일별 포맷 후보를 판별한다.
    Scan {
        root: PathBuf,
        #[arg(long)]
        no_recursive: bool,
        #[arg(long)]
        max_depth: Option<usize>,
        /// 포함 파일명 패턴(`*`, `?`). 여러 번 지정 가능.
        #[arg(long)]
        include: Vec<String>,
        #[arg(long)]
        exclude: Vec<String>,
        /// 파일별로 기본 프리셋 후보 판별까지 수행한다.
        #[arg(long)]
        detect: bool,
        #[arg(long, default_value_t = 200)]
        sample_lines: u64,
    },
    /// 작업 목록과 파일 상태를 출력한다.
    Jobs {
        #[arg(long)]
        db: PathBuf,
    },
    /// 완료된 재파싱 결과를 활성화하고 대체 대상을 비활성화한다.
    Activate {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        job: i64,
    },
    /// 작업 결과(로그·오류·배치)를 삭제한다. 활성 결과는 먼저 다른 결과로 전환해야 한다.
    DeleteResults {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        job: i64,
    },
    /// 등록된 파일이 지금도 같은 내용인지 검증한다.
    Verify {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        source: i64,
        /// 전체 해시까지 비교한다.
        #[arg(long)]
        full: bool,
        /// 파일이 이동됐다면 새 경로로 재연결한 뒤 검증한다.
        #[arg(long)]
        relink: Option<PathBuf>,
    },
    /// 페이지 조회를 반복해 지연을 측정한다.
    Query {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        job: Option<i64>,
        #[arg(long)]
        status: Option<u16>,
        #[arg(long)]
        status_class: Option<u8>,
        #[arg(long)]
        ip: Option<String>,
        #[arg(long)]
        method: Option<String>,
        #[arg(long)]
        target_contains: Option<String>,
        /// 요청 대상 정규식(RE2). 분석 룰 서명 확인용.
        #[arg(long)]
        target_regex: Option<String>,
        /// 시작 시각(UTC 마이크로초).
        #[arg(long)]
        from_micros: Option<i64>,
        #[arg(long)]
        to_micros: Option<i64>,
        #[arg(long, default_value_t = 200)]
        page_size: u32,
        #[arg(long, default_value_t = 3)]
        pages: u32,
        #[arg(long)]
        desc: bool,
        /// 전체 건수도 계산한다(전체 스캔).
        #[arg(long)]
        count: bool,
        #[arg(long)]
        memory_limit: Option<String>,
        #[arg(long)]
        threads: Option<u32>,
    },
    /// 상세 레코드와 재구성 로그를 출력한다.
    Detail {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        job: Option<i64>,
        #[arg(long)]
        source: i64,
        #[arg(long)]
        line: i64,
    },
    /// 파일 선두를 미리보기한다.
    Preview {
        #[arg(long)]
        format: String,
        #[arg(long, default_value_t = 50)]
        lines: u64,
        path: PathBuf,
    },
    /// 조건에 맞는 로그를 CSV/JSON Lines로 스트리밍 내보낸다.
    Export {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        out: PathBuf,
        /// csv 또는 jsonl
        #[arg(long, default_value = "csv")]
        format: String,
        #[arg(long)]
        job: Option<i64>,
        #[arg(long)]
        status: Option<u16>,
        #[arg(long)]
        status_class: Option<u8>,
        #[arg(long)]
        target_contains: Option<String>,
        #[arg(long)]
        from_micros: Option<i64>,
        #[arg(long)]
        to_micros: Option<i64>,
        #[arg(long)]
        max_rows: Option<u64>,
        #[arg(long)]
        memory_limit: Option<String>,
        #[arg(long)]
        threads: Option<u32>,
    },
    /// 기본 통계(상태·메서드·상위 IP/경로·시간축)를 계산하고 소요 시간을 출력한다.
    Analyze {
        #[arg(long)]
        db: PathBuf,
        #[arg(long)]
        job: Option<i64>,
        #[arg(long)]
        from_micros: Option<i64>,
        #[arg(long)]
        to_micros: Option<i64>,
        #[arg(long, default_value_t = 10)]
        top_n: u32,
        #[arg(long)]
        memory_limit: Option<String>,
        #[arg(long)]
        threads: Option<u32>,
    },
    /// 프로필 정의를 검증하거나 YAML로 변환한다.
    Profile {
        #[command(subcommand)]
        action: ProfileAction,
    },
    /// 저장소 메타데이터와 테이블 건수를 출력한다(건수는 전체 스캔).
    Stats {
        #[arg(long)]
        db: PathBuf,
    },
}

fn load_profile(spec: &str) -> Result<FormatProfile> {
    if let Some(p) = presets::by_name(spec) {
        return Ok(p);
    }
    let path = Path::new(spec);
    if path.exists() {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("프로필 읽기 실패: {spec}"))?;
        let is_yaml = matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("yaml" | "yml")
        );
        let profile = if is_yaml {
            weblog_engine::format::yaml::from_yaml(&text)
                .with_context(|| format!("프로필 YAML 해석 실패: {spec}"))?
        } else {
            FormatProfile::from_json(&text)
                .with_context(|| format!("프로필 JSON 파싱 실패: {spec}"))?
        };
        profile
            .ensure_valid()
            .with_context(|| format!("프로필 정의가 유효하지 않음: {spec}"))?;
        return Ok(profile);
    }
    bail!(
        "알 수 없는 포맷 '{spec}'. 프리셋: {}",
        presets::PRESET_NAMES.join(", ")
    );
}

fn store_config(
    memory_limit: Option<String>,
    threads: Option<u32>,
    temp_dir: Option<PathBuf>,
) -> StoreConfig {
    StoreConfig {
        memory_limit,
        threads,
        temp_directory: temp_dir,
        max_temp_directory_size: None,
    }
}

fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Gen {
            format,
            lines,
            seed,
            error_rate,
            blank_rate,
            unique_ips,
            unique_paths,
            gzip,
            out,
        } => {
            let opts = synth::SynthOptions {
                format: match format {
                    GenFormat::Common => synth::SynthFormat::Common,
                    GenFormat::Combined => synth::SynthFormat::Combined,
                    GenFormat::W3c => synth::SynthFormat::W3c,
                    GenFormat::CustomPipe => synth::SynthFormat::CustomPipe,
                },
                lines,
                seed,
                error_rate,
                blank_rate,
                unique_ips,
                unique_paths,
            };
            let started = Instant::now();
            let file = std::fs::File::create(&out)
                .with_context(|| format!("출력 파일 생성 실패: {}", out.display()))?;
            let written = if gzip {
                let mut enc = flate2::write::GzEncoder::new(
                    std::io::BufWriter::new(file),
                    flate2::Compression::default(),
                );
                let n = synth::generate(&opts, &mut enc)?;
                enc.finish()?.into_inner().map_err(|e| e.into_error())?;
                n
            } else {
                let mut w = std::io::BufWriter::with_capacity(1 << 20, file);
                let n = synth::generate(&opts, &mut w)?;
                std::io::Write::flush(&mut w)?;
                n
            };
            if matches!(format, GenFormat::CustomPipe) {
                let profile_path = out.with_extension("profile.json");
                let profile = include_str!("../../../fixtures/custom_pipe.profile.json");
                std::fs::write(&profile_path, profile)?;
                eprintln!("프로필 저장: {}", profile_path.display());
            }
            println!(
                "{}",
                serde_json::json!({
                    "lines": written,
                    "file_bytes": file_size(&out),
                    "gzip": gzip,
                    "elapsed_secs": started.elapsed().as_secs_f64(),
                    "out": out.display().to_string(),
                })
            );
        }
        Command::Import {
            db,
            format,
            batch_rows,
            batch_bytes,
            max_line_bytes,
            memory_limit,
            threads,
            temp_dir,
            progress,
            replaces_job,
            paths,
        } => {
            let profile = load_profile(&format)?;
            let mut store = Store::open(&db, &store_config(memory_limit, threads, temp_dir))?;
            let cfg = ImportConfig {
                batch_max_rows: batch_rows,
                batch_max_bytes: batch_bytes,
                max_line_bytes,
                ..ImportConfig::default()
            };
            let input_bytes: u64 = paths.iter().map(|p| file_size(p)).sum();
            let cancel = Arc::new(AtomicBool::new(false));
            {
                let cancel = Arc::clone(&cancel);
                ctrlc_handler(move || cancel.store(true, Ordering::Relaxed));
            }
            let started = Instant::now();
            let req = ImportRequest {
                profile,
                paths,
                replaces_job_id: replaces_job,
            };
            let summary = run_import(&mut store, &req, &cfg, &cancel, &mut |p| {
                if progress {
                    print_progress(p, started);
                }
            });
            let summary = match summary {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("가져오기 실패: {e}");
                    std::process::exit(2);
                }
            };
            let elapsed = summary.elapsed_secs.max(1e-9);
            let logical: u64 = summary.sources.iter().map(|s| s.logical_bytes).sum();
            println!(
                "{}",
                serde_json::json!({
                    "summary": summary,
                    "input_file_bytes": input_bytes,
                    "logical_bytes": logical,
                    "lines_per_sec": summary.lines_read as f64 / elapsed,
                    "logical_mb_per_sec": logical as f64 / 1e6 / elapsed,
                    "db_file_bytes": file_size(&db),
                    "wal_file_bytes": file_size(&db.with_extension("duckdb.wal")),
                })
            );
        }
        Command::Query {
            db,
            job,
            status,
            status_class,
            ip,
            method,
            target_contains,
            target_regex,
            from_micros,
            to_micros,
            page_size,
            pages,
            desc,
            count,
            memory_limit,
            threads,
        } => {
            let store = Store::open(&db, &store_config(memory_limit, threads, None))?;
            let filter = LogFilter {
                job_id: job,
                source_id: None,
                time_from_micros: from_micros,
                time_to_micros: to_micros,
                status,
                status_class,
                client_ip: ip,
                method,
                target_contains,
                target_regex,
                expr: None,
                active_only: false,
            };
            let mut req = PageRequest {
                filter: filter.clone(),
                sort: if desc {
                    SortOrder::TimeDesc
                } else {
                    SortOrder::TimeAsc
                },
                page_size,
                cursor: None,
            };
            let mut latencies = Vec::new();
            let mut total_rows = 0usize;
            let mut first_row = None;
            for _ in 0..pages {
                let started = Instant::now();
                let page = store.query_page(&req)?;
                latencies.push(started.elapsed().as_secs_f64() * 1000.0);
                total_rows += page.rows.len();
                if first_row.is_none() {
                    first_row = page.rows.first().cloned();
                }
                match page.next_cursor {
                    Some(c) => req.cursor = Some(c),
                    None => break,
                }
            }
            let count_result = if count {
                let started = Instant::now();
                let n = store.count_matching(&filter)?;
                Some((n, started.elapsed().as_secs_f64() * 1000.0))
            } else {
                None
            };
            println!(
                "{}",
                serde_json::json!({
                    "page_latency_ms": latencies,
                    "rows_returned": total_rows,
                    "first_row": first_row,
                    "count": count_result.map(|(n, _)| n),
                    "count_latency_ms": count_result.map(|(_, ms)| ms),
                    "has_more": req.cursor.is_some(),
                })
            );
        }
        Command::Detail {
            db,
            job,
            source,
            line,
        } => {
            let store = Store::open(&db, &StoreConfig::default())?;
            match store.detail(job, source, line)? {
                Some(d) => {
                    // 그 작업이 쓴 프로필 스냅샷의 블록 순서로 재구성한다.
                    let profile = store.profile(store.job(d.job_id)?.profile_id)?;
                    let extra: std::collections::BTreeMap<String, String> = d
                        .extra_json
                        .as_deref()
                        .and_then(|j| serde_json::from_str(j).ok())
                        .unwrap_or_default();
                    let rec = weblog_engine::reconstruct::from_profile(&profile, &d, &extra);
                    println!(
                        "{}",
                        serde_json::json!({
                            "detail": d,
                            "reconstructed_log": rec.text,
                            "is_reconstruction": rec.is_reconstruction,
                            "template": rec.template,
                            "complete": rec.complete,
                            "profile": { "name": profile.name, "version": profile.version },
                        })
                    );
                }
                None => bail!("레코드 없음: source {source} line {line}"),
            }
        }
        Command::Preview {
            format,
            lines,
            path,
        } => {
            let profile = load_profile(&format)?;
            let cfg = PreviewConfig {
                max_lines: lines,
                ..PreviewConfig::default()
            };
            let result = preview_file(&path, &profile, &cfg)?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::Resume {
            db,
            job,
            batch_rows,
            full_verify,
            memory_limit,
            threads,
            progress,
        } => {
            let mut store = Store::open(&db, &store_config(memory_limit, threads, None))?;
            let cfg = ImportConfig {
                batch_max_rows: batch_rows,
                full_verify_on_resume: full_verify,
                ..ImportConfig::default()
            };
            let cancel = Arc::new(AtomicBool::new(false));
            {
                let cancel = Arc::clone(&cancel);
                ctrlc_handler(move || cancel.store(true, Ordering::Relaxed));
            }
            let started = Instant::now();
            match resume_import(&mut store, job, &cfg, &cancel, &mut |p| {
                if progress {
                    print_progress(p, started);
                }
            }) {
                Ok(summary) => println!("{}", serde_json::json!({ "summary": summary })),
                Err(e) => {
                    eprintln!("재개 실패: {e}");
                    std::process::exit(2);
                }
            }
        }
        Command::Scan {
            root,
            no_recursive,
            max_depth,
            include,
            exclude,
            detect,
            sample_lines,
        } => {
            let opts = ScanOptions {
                recursive: !no_recursive,
                max_depth,
                include,
                exclude,
                ..ScanOptions::default()
            };
            let scan = scan_directory(&root, &opts);
            let mut out = serde_json::json!({
                "entries": scan.entries,
                "errors": scan.errors,
                "truncated": scan.truncated,
                "directories_visited": scan.directories_visited,
                "filtered_out": scan.filtered_out,
            });
            if detect {
                let paths: Vec<PathBuf> = scan.entries.iter().map(|e| e.path.clone()).collect();
                let cfg = PreviewConfig {
                    max_lines: sample_lines,
                    ..PreviewConfig::default()
                };
                let (groups, detections, unmatched) =
                    detect_and_group(&paths, &default_candidates(), &cfg)?;
                out["groups"] = serde_json::to_value(groups)?;
                out["detections"] = serde_json::to_value(detections)?;
                out["unmatched"] = serde_json::to_value(unmatched)?;
            }
            println!("{}", serde_json::to_string_pretty(&out)?);
        }
        Command::Jobs { db } => {
            let store = Store::open(&db, &StoreConfig::default())?;
            let mut jobs = Vec::new();
            for j in store.list_jobs()? {
                let sources = store.job_sources(j.job_id)?;
                jobs.push(serde_json::json!({ "job": j, "sources": sources }));
            }
            println!("{}", serde_json::to_string_pretty(&jobs)?);
        }
        Command::Activate { db, job } => {
            let store = Store::open(&db, &StoreConfig::default())?;
            store.activate_job(job)?;
            println!(
                "{}",
                serde_json::json!({ "activated": job, "job": store.job(job)? })
            );
        }
        Command::DeleteResults { db, job } => {
            let store = Store::open(&db, &StoreConfig::default())?;
            let deleted = store.delete_job_results(job)?;
            println!(
                "{}",
                serde_json::json!({ "deleted_job": job, "deleted_log_rows": deleted })
            );
        }
        Command::Verify {
            db,
            source,
            full,
            relink,
        } => {
            let store = Store::open(&db, &StoreConfig::default())?;
            if let Some(p) = relink {
                store.relink_source(source, &p)?;
            }
            let v = store.verify_source(source, full)?;
            println!("{}", serde_json::to_string_pretty(&v)?);
        }
        Command::Export {
            db,
            out,
            format,
            job,
            status,
            status_class,
            target_contains,
            from_micros,
            to_micros,
            max_rows,
            memory_limit,
            threads,
        } => {
            use weblog_engine::export::{export_logs, ExportFormat, ExportRequest};
            let store = Store::open(&db, &store_config(memory_limit, threads, None))?;
            let format = match format.as_str() {
                "csv" => ExportFormat::Csv,
                "jsonl" | "json_lines" => ExportFormat::JsonLines,
                other => bail!("알 수 없는 형식 {other} (csv | jsonl)"),
            };
            let req = ExportRequest {
                filter: LogFilter {
                    job_id: job,
                    time_from_micros: from_micros,
                    time_to_micros: to_micros,
                    status,
                    status_class,
                    target_contains,
                    ..LogFilter::default()
                },
                sort: SortOrder::TimeAsc,
                format,
                out_path: out,
                max_rows,
                include_extra: true,
            };
            let summary = export_logs(&store, &req, &AtomicBool::new(false), &mut |_| {})?;
            let secs = summary.elapsed_secs.max(1e-9);
            println!(
                "{}",
                serde_json::json!({
                    "summary": summary,
                    "rows_per_sec": summary.rows as f64 / secs,
                    "mb_per_sec": summary.bytes as f64 / 1e6 / secs,
                })
            );
        }
        Command::Analyze {
            db,
            job,
            from_micros,
            to_micros,
            top_n,
            memory_limit,
            threads,
        } => {
            use weblog_engine::store::stats::compute_stats;
            use weblog_engine::store::{StatsRequest, TimeBucket};
            let store = Store::open(&db, &store_config(memory_limit, threads, None))?;
            let started = Instant::now();
            let stats = compute_stats(
                &store,
                &StatsRequest {
                    filter: LogFilter {
                        job_id: job,
                        time_from_micros: from_micros,
                        time_to_micros: to_micros,
                        ..LogFilter::default()
                    },
                    top_n,
                    bucket: TimeBucket::Auto,
                    tz_offset_seconds: 0,
                },
            )?;
            println!(
                "{}",
                serde_json::json!({
                    "elapsed_ms": started.elapsed().as_secs_f64() * 1000.0,
                    "total": stats.total,
                    "null_time_rows": stats.null_time_rows,
                    "status": stats.status,
                    "methods": stats.methods,
                    "top_ips": stats.top_ips,
                    "top_targets": stats.top_targets,
                    "timeline_buckets": stats.timeline.len(),
                    "bucket_seconds": stats.bucket_seconds,
                    "timeline_truncated": stats.timeline_truncated,
                })
            );
        }
        Command::Profile { action } => match action {
            ProfileAction::Validate { spec } => {
                let profile = load_profile(&spec)?;
                let issues = profile.validate();
                println!(
                    "{}",
                    serde_json::json!({ "valid": issues.is_empty(), "issues": issues, "definition_hash": profile.definition_hash()? })
                );
            }
            ProfileAction::ToYaml { spec } => {
                let profile = load_profile(&spec)?;
                print!("{}", weblog_engine::format::yaml::to_yaml(&profile)?);
            }
        },
        Command::Stats { db } => {
            let store = Store::open(&db, &StoreConfig::default())?;
            let conn = store.conn_for_tests();
            let mut tables = serde_json::Map::new();
            for t in [
                "sources",
                "parser_profiles",
                "import_jobs",
                "import_batches",
                "logs",
                "parse_errors",
            ] {
                let n: i64 =
                    conn.query_row(&format!("SELECT COUNT(*) FROM {t}"), [], |r| r.get(0))?;
                tables.insert(t.to_owned(), serde_json::json!(n));
            }
            let jobs: Vec<serde_json::Value> = conn
                .prepare("SELECT job_id, result_version, status, committed_records, committed_errors, committed_skipped, failure_reason FROM import_jobs ORDER BY job_id")?
                .query_map([], |r| {
                    Ok(serde_json::json!({
                        "job_id": r.get::<_, i64>(0)?,
                        "result_version": r.get::<_, i64>(1)?,
                        "status": r.get::<_, String>(2)?,
                        "records": r.get::<_, i64>(3)?,
                        "errors": r.get::<_, i64>(4)?,
                        "skipped": r.get::<_, i64>(5)?,
                        "failure_reason": r.get::<_, Option<String>>(6)?,
                    }))
                })?
                .collect::<Result<_, _>>()?;
            println!(
                "{}",
                serde_json::json!({
                    "db_file_bytes": file_size(&db),
                    "wal_file_bytes": file_size(&db.with_extension("duckdb.wal")),
                    "tables": tables,
                    "jobs": jobs,
                })
            );
        }
    }
    Ok(())
}

fn print_progress(p: &weblog_engine::importer::Progress, started: Instant) {
    eprintln!(
        "job {} source {} lines {} committed {} batches {} elapsed {:.1}s",
        p.job_id,
        p.source_id,
        p.lines_read,
        p.committed_records,
        p.committed_batches,
        started.elapsed().as_secs_f64()
    );
}

/// Ctrl+C를 협력적 취소로 연결한다. 시그널 크레이트 없이 표준 라이브러리로는 등록할 수 없으므로
/// 현재는 별도 스레드에서 stdin의 'q' 입력을 취소로 취급한다.
fn ctrlc_handler(cancel: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let mut buf = String::new();
        loop {
            buf.clear();
            match std::io::stdin().read_line(&mut buf) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if buf.trim() == "q" {
                        eprintln!("취소 요청: 현재 배치 이후 중단");
                        cancel();
                        return;
                    }
                }
            }
        }
    });
}
