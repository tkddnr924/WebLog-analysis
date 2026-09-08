//! 결정적 합성 로그 생성기. 같은 줄 복사가 아니라 IP/경로/UA 고유값과 뒤섞인 시간 순서를 포함한다.

use std::fmt::Write as _;

/// 생성 포맷.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SynthFormat {
    /// Apache/Nginx Common.
    Common,
    /// Apache/Nginx Combined.
    Combined,
    /// IIS W3C(중간 헤더 변경 포함).
    W3c,
    /// 파이프 구분 커스텀(`fixtures/custom_pipe.profile.json`과 호환).
    CustomPipe,
}

/// 생성 옵션.
#[derive(Debug, Clone)]
pub struct SynthOptions {
    pub format: SynthFormat,
    pub lines: u64,
    pub seed: u64,
    /// 깨진 줄 비율(0.0~1.0).
    pub error_rate: f64,
    /// 빈 줄 비율.
    pub blank_rate: f64,
    /// 고유 IP 수.
    pub unique_ips: usize,
    /// 고유 경로 수.
    pub unique_paths: usize,
}

struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1) ^ 0x9E37_79B9_7F4A_7C15)
    }
    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next_u64() % n.max(1)
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next_u64() % 1_000_000) as f64 / 1_000_000.0 < p
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64) as usize]
    }
}

const METHODS: &[(&str, u64)] = &[
    ("GET", 80),
    ("POST", 12),
    ("PUT", 3),
    ("DELETE", 2),
    ("HEAD", 3),
];
const STATUSES: &[(u16, u64)] = &[
    (200, 82),
    (304, 5),
    (301, 3),
    (404, 6),
    (403, 1),
    (500, 2),
    (502, 1),
];
const UAS: &[&str] = &[
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36",
    "Mozilla/5.0 (Macintosh; Intel Mac OS X 14_4) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.4 Safari/605.1.15",
    "Mozilla/5.0 (iPhone; CPU iPhone OS 17_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148",
    "Mozilla/5.0 (Linux; Android 14; SM-S918B) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Mobile Safari/537.36",
    "curl/8.6.0",
    "python-requests/2.31.0",
    "Googlebot/2.1 (+http://www.google.com/bot.html)",
    "Mozilla/5.0 (compatible; bingbot/2.0; +http://www.bing.com/bingbot.htm)",
    "okhttp/4.12.0",
    "Go-http-client/2.0",
    "-",
];
const REFERRERS: &[&str] = &[
    "-",
    "-",
    "-",
    "https://www.google.com/",
    "https://example.com/start.html",
    "https://example.com/products?page=2",
    "https://t.co/abc123",
    "https://news.ycombinator.com/",
];
const PATH_STEMS: &[&str] = &[
    "/",
    "/index.html",
    "/api/v1/items",
    "/api/v1/users",
    "/api/v2/search",
    "/static/app.js",
    "/static/style.css",
    "/images/logo.png",
    "/login",
    "/logout",
    "/cart",
    "/checkout",
    "/products",
    "/blog",
    "/health",
    "/wp-admin",
    "/.env",
    "/robots.txt",
];

fn weighted<T: Copy>(rng: &mut Rng, table: &[(T, u64)]) -> T {
    let total: u64 = table.iter().map(|(_, w)| w).sum();
    let mut roll = rng.below(total);
    for (v, w) in table {
        if roll < *w {
            return *v;
        }
        roll -= w;
    }
    table[0].0
}

const MONTHS: &[&str] = &[
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// 초 단위 epoch을 날짜 성분으로. 2024-01-01T00:00:00Z 기준 상대값만 쓴다.
fn civil(secs: i64) -> (i64, u32, u32, u32, u32, u32) {
    // Howard Hinnant의 days_from_civil 역변환.
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        ((rem % 3600) / 60) as u32,
        (rem % 60) as u32,
    )
}

/// 합성 로그를 `out`에 쓴다. 반환값은 생성한 줄 수.
pub fn generate(opts: &SynthOptions, out: &mut dyn std::io::Write) -> std::io::Result<u64> {
    let mut rng = Rng::new(opts.seed);
    let ips: Vec<String> = (0..opts.unique_ips.max(1))
        .map(|i| {
            if i % 20 == 19 {
                format!(
                    "2001:db8:{:x}:{:x}::{:x}",
                    rng.below(0xffff),
                    rng.below(0xffff),
                    rng.below(0xffff)
                )
            } else {
                format!(
                    "{}.{}.{}.{}",
                    1 + rng.below(223),
                    rng.below(256),
                    rng.below(256),
                    1 + rng.below(254)
                )
            }
        })
        .collect();
    let paths: Vec<String> = (0..opts.unique_paths.max(1))
        .map(|i| {
            let stem = PATH_STEMS[i % PATH_STEMS.len()];
            match rng.below(4) {
                0 => stem.to_owned(),
                1 => format!("{stem}/{}", rng.below(100_000)),
                2 => format!("{stem}?id={}&q=%EC%84%9C%EC%9A%B8", rng.below(10_000)),
                _ => format!("{stem}?page={}&sort=desc", rng.below(500)),
            }
        })
        .collect();
    // 2024-01-01T00:00:00Z 기준. CLF는 +0900로 기록해 UTC 변환을 검증한다.
    let base_utc: i64 = 1_704_067_200;
    let mut clock = base_utc;
    let mut written = 0u64;
    let mut line = String::with_capacity(512);

    let w3c_fields_a = "#Fields: date time s-ip cs-method cs-uri-stem cs-uri-query s-port cs-username c-ip cs(User-Agent) cs(Referer) sc-status sc-substatus sc-win32-status time-taken";
    let w3c_fields_b =
        "#Fields: date time c-ip cs-method cs-uri-stem cs-uri-query sc-status sc-bytes time-taken";
    if opts.format == SynthFormat::W3c {
        writeln!(
            out,
            "#Software: Microsoft Internet Information Services 10.0"
        )?;
        writeln!(out, "#Version: 1.0")?;
        writeln!(out, "#Date: 2024-01-01 00:00:00")?;
        writeln!(out, "{w3c_fields_a}")?;
    }
    let half = opts.lines / 2;
    for i in 0..opts.lines {
        line.clear();
        if opts.format == SynthFormat::W3c && i == half {
            writeln!(out, "{w3c_fields_b}")?;
        }
        // 기준 시계는 단조 증가(평균 1초/줄)하고, 5%의 줄은 최대 60초 늦게 기록된 것처럼 뒤로 간다.
        // 뒤로 간 값이 기준 시계를 바꾸지 않으므로 평균 드리프트 없이 순서만 섞인다.
        clock += rng.below(3) as i64;
        let t = if rng.chance(0.05) {
            clock - rng.below(60) as i64
        } else {
            clock
        };
        if rng.chance(opts.blank_rate) {
            writeln!(out)?;
            written += 1;
            continue;
        }
        if rng.chance(opts.error_rate) {
            writeln!(
                out,
                "corrupt line {} <<>> {}",
                rng.next_u64(),
                rng.next_u64()
            )?;
            written += 1;
            continue;
        }
        let ip = rng.pick(&ips);
        let path = rng.pick(&paths);
        let method = weighted(&mut rng, METHODS);
        let status = weighted(&mut rng, STATUSES);
        let bytes = if rng.chance(0.03) {
            None
        } else {
            Some(rng.below(200_000))
        };
        let ua = rng.pick(UAS);
        let referrer = rng.pick(REFERRERS);
        let bytes_str = bytes.map_or("-".to_owned(), |b| b.to_string());
        match opts.format {
            SynthFormat::Common | SynthFormat::Combined => {
                let (y, mo, d, h, mi, s) = civil(t + 9 * 3600);
                let _ = write!(
                    line,
                    "{ip} - - [{d:02}/{}/{y}:{h:02}:{mi:02}:{s:02} +0900] \"{method} {path} HTTP/1.1\" {status} {bytes_str}",
                    MONTHS[(mo - 1) as usize]
                );
                if opts.format == SynthFormat::Combined {
                    let _ = write!(line, " \"{referrer}\" \"{ua}\"");
                }
            }
            SynthFormat::W3c => {
                let (y, mo, d, h, mi, s) = civil(t);
                let (stem, query) = match path.split_once('?') {
                    Some((a, b)) => (a, b),
                    None => (path.as_str(), "-"),
                };
                let ua_w3c = ua.replace(' ', "+");
                if i < half {
                    let _ = write!(
                        line,
                        "{y}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} 10.0.0.1 {method} {stem} {query} 443 - {ip} {ua_w3c} {referrer} {status} 0 0 {}",
                        rng.below(2000)
                    );
                } else {
                    let _ = write!(
                        line,
                        "{y}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} {ip} {method} {stem} {query} {status} {bytes_str} {}",
                        rng.below(2000)
                    );
                }
            }
            SynthFormat::CustomPipe => {
                let (y, mo, d, h, mi, s) = civil(t + 9 * 3600);
                let _ = write!(
                    line,
                    "{y}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}|{ip}|{method}|{path}|{status}|{bytes_str}|{ua}|rt={}",
                    rng.below(5000)
                );
            }
        }
        out.write_all(line.as_bytes())?;
        out.write_all(b"\n")?;
        written += 1;
    }
    Ok(written)
}
