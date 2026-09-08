// 퍼즐 매핑: 샘플 한 줄을 조각으로 나누고, 조각마다 붙인 라벨(서버별 로그 변수)로 프로필 정의를 만든다. 순수 함수, 테스트 대상.
import type { Block, Capture, FieldKind, FormatProfile, ServerHint, TimestampFormat } from "../types";

export type Separator = "space" | "|" | "," | ";" | "tab";

export const SEPARATORS: { id: Separator; label: string; char: string }[] = [
  { id: "space", label: "공백", char: " " },
  { id: "|", label: "| (파이프)", char: "|" },
  { id: ",", label: ", (쉼표)", char: "," },
  { id: ";", label: "; (세미콜론)", char: ";" },
  { id: "tab", label: "탭", char: "\t" },
];

/** 라벨 식별자. 서버별 어휘(vocab)의 id다. */
export type Role = string;

export type RoleGroup = "client" | "time" | "request" | "response" | "server" | "misc";

export const GROUP_LABELS: Record<RoleGroup, string> = {
  client: "클라이언트",
  time: "시간",
  request: "요청",
  response: "응답",
  server: "서버·업스트림",
  misc: "기타",
};

export interface RoleDef {
  id: Role;
  /** 조각 위에 보이는 라벨(변수명 또는 지시자). */
  label: string;
  hint: string;
  group: RoleGroup;
  /** 엔진 필드 종류. "ignore"는 저장하지 않는다. */
  kind: FieldKind["kind"] | "ignore";
  /** 저장 필드 이름. 확장 필드는 이 이름으로 저장된다. */
  name?: string;
  /** 시간 라벨의 고정 형식. 없으면 값 모양으로 추정한다. */
  ts?: TimestampFormat;
  /** 이 조각부터 줄 끝까지 전부 가져간다(메시지 등). 뒤 조각은 흡수된다. */
  rest?: boolean;
  /** 줄 끝까지 가져가되 메시지 뒤에 `, key: value` 꼴로 붙는 항목을 각각 필드로 나눈다(있을 때만). */
  tail?: TailItem[];
  /** 자주 쓰지 않는 라벨. 팔레트에서 "그 외"로 내려간다. */
  rare?: boolean;
}

export type LogKind = "access" | "error";

/** 메시지 뒤에 선택적으로 붙는 `, key: value` 항목. 값이 따옴표로 감싸이면 quoted. */
export interface TailItem {
  key: string;
  label: string;
  kind: FieldKind["kind"];
  name: string;
  quoted: boolean;
}

const DEFAULT_TS: TimestampFormat = { kind: "clf" };

type Row = [Role, string, string, RoleGroup, FieldKind["kind"] | "ignore", string?, TimestampFormat?];

/** 라벨은 사람이 읽는 말, 변수명은 툴팁과 저장 이름에만 쓴다. */
function rows(list: Row[]): RoleDef[] {
  return list.map(([id, label, hint, group, kind, name, ts]) => ({ id, label, hint: `${id} · ${hint}`, group, kind, name, ts }));
}

/** 모든 어휘의 끝에 붙는 공통 항목. */
const GENERIC_TAIL: RoleDef[] = [
  { id: "integer", label: "기타 숫자", hint: "이름 없는 정수. 위치 번호로 확장 필드에 저장", group: "misc", kind: "integer" },
  { id: "text", label: "기타 문자열", hint: "이름 없는 문자열. 위치 번호로 확장 필드에 저장", group: "misc", kind: "text" },
  { id: "ignore", label: "무시", hint: "저장하지 않음", group: "misc", kind: "ignore" },
];

/** Nginx log_format 변수. */
const NGINX: RoleDef[] = [
  ...rows([
    ["$remote_addr", "클라이언트 IP", "클라이언트 IP 주소", "client", "client_ip", "client_ip"],
    ["$remote_port", "클라이언트 포트", "클라이언트 포트 번호", "client", "integer", "remote_port"],
    ["$remote_user", "로그인 사용자", "Basic 인증 사용자 이름", "client", "text", "remote_user"],
    ["$http_x_forwarded_for", "프록시 원본 IP", "X-Forwarded-For 헤더", "client", "text", "x_forwarded_for"],
    ["$http_x_real_ip", "실제 IP 헤더", "X-Real-IP 헤더", "client", "text", "x_real_ip"],
    ["$time_local", "요청 시각", "로컬 시간대 (26/Feb/2025:15:20:45 +0000)", "time", "timestamp", "timestamp", { kind: "clf" }],
    ["$time_iso8601", "요청 시각(ISO)", "ISO 8601 (2025-02-26T15:20:45+00:00)", "time", "timestamp", "timestamp", { kind: "iso8601" }],
    ["$msec", "요청 시각(초.밀리초)", "유닉스 초.밀리초", "time", "text", "msec"],
    ["$request_time", "처리 시간(초)", "요청 처리에 걸린 시간", "time", "text", "request_time"],
    ["$request", "요청 라인", "GET /index.html HTTP/1.1", "request", "request_line", "request"],
    ["$request_method", "요청 방식", "GET, POST…", "request", "method", "method"],
    ["$request_uri", "요청 URI", "쿼리 포함 (/index.html?query=123)", "request", "request_target", "request_target"],
    ["$uri", "경로", "쿼리를 뺀 경로", "request", "text", "uri"],
    ["$document_uri", "경로(document)", "$uri와 동일", "request", "text", "document_uri"],
    ["$query_string", "쿼리 문자열", "query=123", "request", "text", "query_string"],
    ["$args", "쿼리 문자열(args)", "$query_string과 동일", "request", "text", "args"],
    ["$request_length", "요청 크기", "요청 바이트 크기", "request", "integer", "request_length"],
    ["$server_protocol", "프로토콜", "HTTP/1.1", "request", "protocol", "protocol"],
    ["$scheme", "스킴", "http 또는 https", "request", "text", "scheme"],
    ["$host", "호스트", "요청의 호스트 이름", "request", "text", "host"],
    ["$http_host", "Host 헤더", "Host 헤더 값", "request", "text", "http_host"],
    ["$http_referer", "리퍼러", "Referer 헤더", "request", "referrer", "referrer"],
    ["$http_user_agent", "브라우저(UA)", "User-Agent 헤더", "request", "user_agent", "user_agent"],
    ["$http_cookie", "쿠키", "Cookie 헤더", "request", "text", "cookie"],
    ["$status", "상태코드", "응답 상태코드", "response", "status", "status"],
    ["$body_bytes_sent", "응답 크기", "본문 바이트(헤더 제외)", "response", "bytes_sent", "bytes_sent"],
    ["$bytes_sent", "전송 크기(헤더 포함)", "전송 바이트", "response", "integer", "bytes_sent_total"],
    ["$gzip_ratio", "압축률", "gzip 압축률", "response", "text", "gzip_ratio"],
    ["$server_name", "서버 이름", "server_name 값", "server", "text", "server_name"],
    ["$server_addr", "서버 주소", "서버 IP", "server", "text", "server_addr"],
    ["$server_port", "서버 포트", "서버 포트 번호", "server", "integer", "server_port"],
    ["$upstream_addr", "업스트림 주소", "백엔드 주소", "server", "text", "upstream_addr"],
    ["$upstream_status", "업스트림 상태코드", "백엔드 응답 상태", "server", "text", "upstream_status"],
    ["$upstream_response_time", "업스트림 응답 시간", "초 단위", "server", "text", "upstream_response_time"],
    ["$upstream_connect_time", "업스트림 연결 시간", "초 단위", "server", "text", "upstream_connect_time"],
    ["$upstream_header_time", "업스트림 헤더 시간", "초 단위", "server", "text", "upstream_header_time"],
    ["$upstream_cache_status", "캐시 상태", "HIT, MISS…", "server", "text", "upstream_cache_status"],
    ["$connection", "연결 번호", "연결 일련번호", "misc", "integer", "connection"],
    ["$connection_requests", "연결의 요청 수", "같은 연결에서 몇 번째 요청인지", "misc", "integer", "connection_requests"],
    ["$pipe", "파이프라인", "p 또는 .", "misc", "text", "pipe"],
    ["$ssl_protocol", "TLS 버전", "TLS 프로토콜", "misc", "text", "ssl_protocol"],
    ["$ssl_cipher", "TLS 암호", "TLS 암호 스위트", "misc", "text", "ssl_cipher"],
    ["$request_id", "요청 ID", "요청 식별자", "misc", "text", "request_id"],
    ["$hostname", "서버 호스트명", "서버 호스트 이름", "misc", "text", "hostname"],
    ["$pid", "프로세스 ID", "워커 프로세스 ID", "misc", "integer", "pid"],
  ]),
  ...GENERIC_TAIL,
];

/** Apache mod_log_config 지시자. */
const APACHE: RoleDef[] = [
  ...rows([
    ["%h", "클라이언트 IP", "원격 호스트(보통 클라이언트 IP)", "client", "client_ip", "client_ip"],
    ["%a", "클라이언트 IP(원본)", "%h와 같을 때가 많음", "client", "text", "peer_ip"],
    ["%l", "identd", "identd 응답, 보통 -", "client", "text", "ident"],
    ["%u", "로그인 사용자", "인증 사용자 이름", "client", "text", "remote_user"],
    ["%{X-Forwarded-For}i", "프록시 원본 IP", "X-Forwarded-For 헤더", "client", "text", "x_forwarded_for"],
    ["%t", "요청 시각", "[10/Oct/2000:13:55:36 -0700]", "time", "timestamp", "timestamp", { kind: "clf" }],
    ["%D", "처리 시간(μs)", "마이크로초", "time", "integer", "duration_us"],
    ["%T", "처리 시간(초)", "초", "time", "integer", "duration_s"],
    ["%{ms}T", "처리 시간(ms)", "밀리초", "time", "integer", "duration_ms"],
    ["%r", "요청 라인", "GET /index.html HTTP/1.1", "request", "request_line", "request"],
    ["%m", "요청 방식", "GET, POST…", "request", "method", "method"],
    ["%U", "경로", "쿼리를 뺀 경로", "request", "request_target", "request_target"],
    ["%q", "쿼리 문자열", "?query=123", "request", "text", "query_string"],
    ["%H", "프로토콜", "HTTP/1.1", "request", "protocol", "protocol"],
    ["%{Referer}i", "리퍼러", "Referer 헤더", "request", "referrer", "referrer"],
    ["%{User-Agent}i", "브라우저(UA)", "User-Agent 헤더", "request", "user_agent", "user_agent"],
    ["%{Host}i", "Host 헤더", "Host 헤더 값", "request", "text", "host"],
    ["%f", "파일 이름", "요청된 파일", "request", "text", "filename"],
    ["%R", "핸들러", "요청을 처리한 핸들러", "request", "text", "handler"],
    ["%>s", "상태코드", "최종 상태코드", "response", "status", "status"],
    ["%s", "최초 상태코드", "내부 리다이렉트 전 상태", "response", "integer", "status_first"],
    ["%b", "응답 크기", "응답 바이트(없으면 -)", "response", "bytes_sent", "bytes_sent"],
    ["%B", "응답 크기(0 표기)", "응답 바이트(없으면 0)", "response", "integer", "bytes_body"],
    ["%O", "보낸 크기(헤더 포함)", "보낸 바이트", "response", "integer", "bytes_out"],
    ["%I", "받은 크기(헤더 포함)", "받은 바이트", "response", "integer", "bytes_in"],
    ["%S", "송수신 합계", "주고받은 바이트 합", "response", "integer", "bytes_transferred"],
    ["%v", "서버 이름", "ServerName", "server", "text", "server_name"],
    ["%V", "정식 서버 이름", "UseCanonicalName 기준", "server", "text", "server_name_canonical"],
    ["%p", "서버 포트", "서버 포트 번호", "server", "integer", "server_port"],
    ["%A", "서버 IP", "로컬 IP", "server", "text", "local_ip"],
    ["%P", "프로세스 ID", "요청을 처리한 프로세스", "server", "integer", "pid"],
    ["%L", "로그 ID", "오류 로그 요청 ID", "misc", "text", "log_id"],
    ["%k", "keepalive 순번", "같은 연결의 요청 순번", "misc", "integer", "keepalive"],
  ]),
  ...GENERIC_TAIL,
];

/** 서버를 모를 때의 일반 라벨. */
const GENERIC: RoleDef[] = [
  { id: "timestamp", label: "시간", hint: "요청 시각", group: "time", kind: "timestamp", name: "timestamp" },
  { id: "client_ip", label: "IP", hint: "클라이언트 주소", group: "client", kind: "client_ip", name: "client_ip" },
  { id: "remote_user", label: "사용자", hint: "인증 사용자", group: "client", kind: "text", name: "remote_user" },
  { id: "ident", label: "identd", hint: "identd 응답, 보통 -", group: "client", kind: "text", name: "ident" },
  { id: "x_forwarded_for", label: "X-Forwarded-For", hint: "프록시 뒤 원래 주소", group: "client", kind: "text", name: "x_forwarded_for" },
  { id: "request_line", label: "요청문", hint: "\"GET /path HTTP/1.1\" 한 덩어리", group: "request", kind: "request_line", name: "request" },
  { id: "method", label: "메서드", hint: "GET, POST…", group: "request", kind: "method", name: "method" },
  { id: "request_target", label: "URL", hint: "경로와 쿼리", group: "request", kind: "request_target", name: "request_target" },
  { id: "protocol", label: "프로토콜", hint: "HTTP/1.1", group: "request", kind: "protocol", name: "protocol" },
  { id: "host", label: "호스트", hint: "서버 이름", group: "request", kind: "text", name: "host" },
  { id: "referrer", label: "리퍼러", hint: "Referer 헤더", group: "request", kind: "referrer", name: "referrer" },
  { id: "user_agent", label: "UA", hint: "User-Agent 헤더", group: "request", kind: "user_agent", name: "user_agent" },
  { id: "status", label: "상태코드", hint: "200, 404…", group: "response", kind: "status", name: "status" },
  { id: "bytes_sent", label: "바이트", hint: "응답 크기", group: "response", kind: "bytes_sent", name: "bytes_sent" },
  { id: "request_time", label: "응답 시간", hint: "처리에 걸린 시간", group: "response", kind: "text", name: "request_time" },
  ...GENERIC_TAIL,
];

const MESSAGE_REST: RoleDef = { id: "message", label: "메시지(줄 끝까지)", hint: "이 조각부터 줄 끝까지 전부 메시지로 저장", group: "misc", kind: "text", name: "message", rest: true };

/** Nginx 에러 로그 메시지 꼬리(ngx_http 로그 컨텍스트 순서): client, server, request, upstream, host, referrer. 있을 때만 붙는다. */
export const NGINX_ERROR_TAIL: TailItem[] = [
  { key: "client", label: "클라이언트 IP", kind: "client_ip", name: "client_ip", quoted: false },
  { key: "server", label: "서버", kind: "text", name: "server", quoted: false },
  { key: "request", label: "요청 라인", kind: "request_line", name: "request", quoted: true },
  { key: "upstream", label: "업스트림", kind: "text", name: "upstream", quoted: true },
  { key: "host", label: "호스트", kind: "text", name: "host", quoted: true },
  { key: "referrer", label: "리퍼러", kind: "referrer", name: "referrer", quoted: true },
];

/**
 * Nginx error_log 한 줄(ngx_log 고정 형식): `YYYY/MM/DD HH:MM:SS [level] pid#tid: *cid message, client: …`.
 * 레벨은 debug·info·notice·warn·error·crit·alert·emerg.
 */
const NGINX_ERROR: RoleDef[] = [
  { id: "err_time", label: "발생 시각", hint: "2026/09/06 00:49:09 (서버 로컬 시간, 오프셋 없음)", group: "time", kind: "timestamp", name: "timestamp", ts: { kind: "custom", pattern: "%Y/%m/%d %H:%M:%S" } },
  { id: "err_level", label: "레벨", hint: "[debug] [info] [notice] [warn] [error] [crit] [alert] [emerg]", group: "response", kind: "text", name: "level" },
  { id: "err_pid", label: "프로세스#스레드", hint: "워커 PID#TID (1234#0:)", group: "server", kind: "text", name: "pid_tid" },
  { id: "err_conn", label: "연결 번호", hint: "*5 — 접근 로그의 $connection과 같은 값", group: "server", kind: "text", name: "connection" },
  {
    id: "message_detail",
    label: "메시지 + 상세 분리",
    hint: "줄 끝까지 가져가되 뒤에 붙는 client(IP)·server·request·upstream·host·referrer를 각각 나눠 저장",
    group: "misc",
    kind: "text",
    name: "message",
    rest: true,
    tail: NGINX_ERROR_TAIL,
  },
  MESSAGE_REST,
  ...GENERIC_TAIL,
];

/** Apache 에러 로그 꼬리: 요청 관련 오류에는 `, referer: URL`이 붙을 수 있다. */
export const APACHE_ERROR_TAIL: TailItem[] = [{ key: "referer", label: "리퍼러", kind: "referrer", name: "referrer", quoted: false }];

/** Apache error_log 한 줄. 2.4 기본 ErrorLogFormat 기준이며 %E(APR/OS 오류)·%F(소스 파일:줄)·%L(요청 ID)은 설정에 따라 붙는다. */
const APACHE_ERROR: RoleDef[] = [
  { id: "err_time", label: "발생 시각", hint: "[Sun Sep 06 00:49:09.123456 2026] (%{u}t, 마이크로초 포함)", group: "time", kind: "timestamp", name: "timestamp", ts: { kind: "custom", pattern: "%a %b %d %H:%M:%S%.f %Y" } },
  { id: "err_time_s", label: "발생 시각(초)", hint: "[Sun Sep 06 00:49:09 2026] (%t, 마이크로초 없음)", group: "time", kind: "timestamp", name: "timestamp", ts: { kind: "custom", pattern: "%a %b %d %H:%M:%S %Y" } },
  { id: "err_level", label: "모듈:레벨", hint: "[core:error] (%-m:%l) — emerg·alert·crit·error·warn·notice·info·debug·trace1~8", group: "response", kind: "text", name: "level" },
  { id: "err_pid", label: "프로세스/스레드", hint: "[pid 123:tid 456] (%P:%T)", group: "server", kind: "text", name: "pid_tid" },
  { id: "err_client", label: "클라이언트", hint: "[client 1.2.3.4:5678] (%a) — 요청과 관련된 오류에만 붙음", group: "client", kind: "text", name: "client" },
  { id: "err_reqid", label: "요청 ID", hint: "[%L] — 접근 로그 %L과 대응", group: "server", kind: "text", name: "log_id" },
  { id: "err_file", label: "소스 파일:줄", hint: "[%F] — 예: mod_ssl.c(2321)", group: "server", kind: "text", name: "source_file" },
  { id: "err_apr", label: "APR/OS 오류", hint: "(%E) — 예: (2)No such file or directory", group: "response", kind: "text", name: "os_error" },
  { id: "err_code", label: "오류 코드", hint: "AH00126 같은 메시지 앞 코드", group: "response", kind: "text", name: "error_code" },
  {
    id: "message_detail",
    label: "메시지 + 리퍼러 분리",
    hint: "줄 끝까지 가져가되 뒤에 붙는 referer를 나눠 저장",
    group: "misc",
    kind: "text",
    name: "message",
    rest: true,
    tail: APACHE_ERROR_TAIL,
  },
  MESSAGE_REST,
  ...GENERIC_TAIL,
];

/** 서버를 모를 때의 에러 로그 라벨. */
const GENERIC_ERROR: RoleDef[] = [
  { id: "err_time", label: "발생 시각", hint: "날짜와 시각", group: "time", kind: "timestamp", name: "timestamp" },
  { id: "err_level", label: "레벨", hint: "error, warn…", group: "response", kind: "text", name: "level" },
  { id: "err_source", label: "출처", hint: "프로세스, 모듈, 클라이언트 등", group: "server", kind: "text", name: "source" },
  { id: "client_ip", label: "클라이언트 IP", hint: "IP 주소", group: "client", kind: "client_ip", name: "client_ip" },
  { id: "err_code", label: "오류 코드", hint: "메시지 앞 코드", group: "response", kind: "text", name: "error_code" },
  MESSAGE_REST,
  ...GENERIC_TAIL,
];

/** 자주 쓰는 라벨. 나머지는 팔레트에서 "그 외"로 내려간다. */
const COMMON_IDS = new Set<string>([
  // nginx access
  "$remote_addr", "$remote_user", "$time_local", "$time_iso8601", "$request", "$request_method", "$request_uri", "$status", "$body_bytes_sent",
  "$http_referer", "$http_user_agent", "$http_x_forwarded_for", "$request_time", "$upstream_response_time", "$host", "$server_protocol",
  // apache access
  "%h", "%l", "%u", "%t", "%r", "%>s", "%b", "%{Referer}i", "%{User-Agent}i", "%D", "%{X-Forwarded-For}i", "%v", "%m", "%U", "%H",
  // error logs
  "err_time", "err_time_s", "err_level", "err_pid", "err_conn", "err_client", "err_code", "err_source", "message_detail", "message",
  // 공통
  "timestamp", "client_ip", "remote_user", "ident", "x_forwarded_for", "request_line", "method", "request_target", "protocol", "host",
  "referrer", "user_agent", "status", "bytes_sent", "request_time", "ignore",
]);

function markRare(list: RoleDef[]): RoleDef[] {
  return list.map((r) => (COMMON_IDS.has(r.id) ? r : { ...r, rare: true }));
}

/** 서버 종류·로그 종류별 라벨 어휘. IIS 접근 로그는 헤더 기반이라 퍼즐을 쓰지 않으므로 일반 어휘를 돌려준다. */
export function vocabFor(server: ServerHint, kind: LogKind = "access"): RoleDef[] {
  if (kind === "error") {
    if (server === "nginx") return markRare(NGINX_ERROR);
    if (server === "apache") return markRare(APACHE_ERROR);
    return markRare(GENERIC_ERROR);
  }
  switch (server) {
    case "nginx":
      return markRare(NGINX);
    case "apache":
      return markRare(APACHE);
    default:
      return markRare(GENERIC);
  }
}

export function roleDef(vocab: RoleDef[], role: Role): RoleDef | undefined {
  return vocab.find((r) => r.id === role);
}

export function roleLabel(vocab: RoleDef[], role: Role): string {
  return roleDef(vocab, role)?.label ?? role;
}

/** 어휘 안에서 필드 종류에 맞는 라벨. 이름이 맞는 것을 우선하고, 시간은 형식이 맞는 것을 우선한다. */
export function roleForKind(vocab: RoleDef[], kind: FieldKind, name?: string): Role {
  const same = vocab.filter((r) => r.kind === kind.kind);
  if (name) {
    const byName = same.find((r) => r.name === name);
    if (byName) return byName.id;
  }
  if (kind.kind === "timestamp") {
    const byTs = same.find((r) => r.ts?.kind === kind.format.kind);
    if (byTs) return byTs.id;
  }
  if (kind.kind === "text" || kind.kind === "integer") return kind.kind;
  return same[0]?.id ?? "text";
}

/** 내장 프리셋 이름의 설명. 이름만으로는 뜻을 알기 어렵다. */
export const PRESET_LABELS: Record<string, string> = {
  common: "Apache/Nginx 기본(common): IP · 시간 · 요청 · 상태 · 바이트",
  combined: "Apache/Nginx 확장(combined): 기본 + 리퍼러 · UA",
  apache_combined: "Apache combined",
  nginx_combined: "Nginx combined",
  iis_w3c: "IIS W3C (#Fields 헤더 기반)",
};

export interface Piece {
  /** 구분 기호를 포함한 원문 조각. */
  text: string;
  /** 따옴표·대괄호를 뺀 값. */
  value: string;
  /** 엔진 추출 방식. */
  capture: Capture["kind"];
}

/** 조각 하나의 라벨. 시간 조각은 형식을 함께 가진다(어휘에 고정 형식이 없을 때 쓴다). */
export interface RoleAssign {
  role: Role;
  tsFormat: TimestampFormat;
}

/** 한 줄을 조각으로 나눈다. 공백 구분은 "…"와 […]를 한 조각으로 본다. */
export function tokenize(line: string, sep: Separator): Piece[] {
  if (sep !== "space") {
    const ch = SEPARATORS.find((s) => s.id === sep)?.char ?? sep;
    return line.split(ch).map((t) => ({ text: t, value: t, capture: "pattern" }));
  }
  const out: Piece[] = [];
  let i = 0;
  const n = line.length;
  while (i < n) {
    const c = line[i];
    if (c === " " || c === "\t") {
      i += 1;
      continue;
    }
    if (c === '"') {
      let j = i + 1;
      while (j < n && line[j] !== '"') j += line[j] === "\\" ? 2 : 1;
      const end = Math.min(j + 1, n);
      out.push({ text: line.slice(i, end), value: line.slice(i + 1, j), capture: "quoted" });
      i = end;
      continue;
    }
    if (c === "[") {
      const j = line.indexOf("]", i + 1);
      const end = j === -1 ? n : j + 1;
      out.push({ text: line.slice(i, end), value: line.slice(i + 1, j === -1 ? n : j), capture: "bracketed" });
      i = end;
      continue;
    }
    let j = i;
    while (j < n && line[j] !== " " && line[j] !== "\t") j += 1;
    out.push({ text: line.slice(i, j), value: line.slice(i, j), capture: "token" });
    i = j;
  }
  return mergeDateTime(out);
}

const DATE_ONLY = /^\d{4}[/-]\d{2}[/-]\d{2}$/;
const TIME_ONLY = /^\d{2}:\d{2}:\d{2}(\.\d+)?,?$/;

/** `2026/09/06 00:49:09`처럼 공백으로 나뉜 날짜와 시각은 한 조각으로 본다. */
function mergeDateTime(pieces: Piece[]): Piece[] {
  const out: Piece[] = [];
  for (let i = 0; i < pieces.length; i += 1) {
    const a = pieces[i];
    const b = pieces[i + 1];
    if (a.capture === "token" && b?.capture === "token" && DATE_ONLY.test(a.value) && TIME_ONLY.test(b.value)) {
      const text = `${a.text} ${b.text}`;
      out.push({ text, value: text, capture: "pattern" });
      i += 1;
    } else out.push(a);
  }
  return out;
}

/** 프리셋/정의에 고정 문자열 구분자가 있으면 그 구분자, 아니면 공백. W3C면 null. */
export function separatorOf(profile: FormatProfile): Separator | null {
  if (profile.strategy.kind !== "blocks") return null;
  for (const b of profile.strategy.blocks) {
    if (b.block === "literal") {
      const s = SEPARATORS.find((x) => x.char === b.text);
      if (s) return s.id;
    }
  }
  return "space";
}

/** 줄 모양으로 구분자를 추정한다. */
export function guessSeparator(line: string): Separator {
  const count = (ch: string) => line.split(ch).length - 1;
  if (count("|") >= 3) return "|";
  if (count("\t") >= 3) return "tab";
  if (count(" ") === 0 && count(",") >= 3) return ",";
  if (count(" ") === 0 && count(";") >= 3) return ";";
  return "space";
}

const ISO_TS = /^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}/;
const CLF_TS = /^\d{1,2}\/[A-Za-z]{3}\/\d{4}:\d{2}:\d{2}:\d{2}/;
const IPV4 = /^\d{1,3}(\.\d{1,3}){3}$/;
const IPV6 = /^[0-9a-f:]+:[0-9a-f:]*$/i;
const METHOD = /^(GET|POST|PUT|DELETE|HEAD|OPTIONS|PATCH|CONNECT|TRACE)$/;

/** 값 모양으로 시간 형식을 추정한다. 모르면 CLF. */
export function inferTsFormat(value: string): TimestampFormat {
  if (ISO_TS.test(value)) return { kind: "iso8601" };
  return { kind: "clf" };
}

/** 정의의 최상위 필드 순서를 조각에 차례로 대응시킨다. 남는 조각은 무시, 부족하면 있는 만큼만. W3C면 null. */
export function rolesFromProfile(profile: FormatProfile, pieces: Piece[], vocab: RoleDef[]): RoleAssign[] | null {
  if (profile.strategy.kind !== "blocks") return null;
  const fields: RoleAssign[] = [];
  for (const b of profile.strategy.blocks) {
    if (b.block === "field") {
      if (b.capture.kind === "pattern" && (b.capture.pattern === ".*" || b.capture.pattern === ".*?")) {
        const wantTail = b.capture.pattern === ".*?";
        const rest = vocab.find((r) => r.rest && Boolean(r.tail?.length) === wantTail) ?? vocab.find((r) => r.rest);
        if (rest) {
          fields.push({ role: rest.id, tsFormat: DEFAULT_TS });
          break;
        }
      }
      fields.push({ role: roleForKind(vocab, b.kind, b.name), tsFormat: b.kind.kind === "timestamp" ? b.kind.format : DEFAULT_TS });
    } else if (b.block === "regex") fields.push({ role: "ignore", tsFormat: DEFAULT_TS });
    else if (b.block === "optional_group") break;
  }
  return pieces.map((_, i) => fields[i] ?? { role: "ignore", tsFormat: DEFAULT_TS });
}

const LEVEL = /^\[?(emerg|alert|crit|error|warn|warning|notice|info|debug)\]?$/i;
const MODULE_LEVEL = /^[a-z_]+:(emerg|alert|crit|error|warn|notice|info|debug|trace\d?)$/i;
const WEEKDAY_TS = /^(Mon|Tue|Wed|Thu|Fri|Sat|Sun) [A-Z][a-z]{2} \d{1,2} \d{2}:\d{2}:\d{2}/;

/** 에러 로그 줄의 라벨 추정. 시각·레벨·출처 뒤는 전부 메시지다. */
export function guessErrorRoles(pieces: Piece[], vocab: RoleDef[]): RoleAssign[] {
  const byName = (name: string) => vocab.find((r) => r.name === name)?.id;
  const rest = vocab.find((r) => r.rest)?.id ?? "text";
  const timeRole = (v: string): RoleAssign => {
    if (WEEKDAY_TS.test(v)) {
      const frac = /\d{2}:\d{2}:\d{2}\.\d+/.test(v);
      const id = frac ? byName("timestamp") : (vocab.find((r) => r.id === "err_time_s")?.id ?? byName("timestamp"));
      return { role: id ?? "text", tsFormat: { kind: "custom", pattern: frac ? "%a %b %d %H:%M:%S%.f %Y" : "%a %b %d %H:%M:%S %Y" } };
    }
    if (ISO_TS.test(v)) return { role: byName("timestamp") ?? "text", tsFormat: { kind: "iso8601" } };
    return { role: byName("timestamp") ?? "text", tsFormat: { kind: "custom", pattern: "%Y/%m/%d %H:%M:%S" } };
  };
  const out: RoleAssign[] = [];
  let inMessage = false;
  for (const p of pieces) {
    const v = p.value;
    if (inMessage) {
      out.push({ role: "ignore", tsFormat: DEFAULT_TS });
      continue;
    }
    if (out.length === 0 && (DATE_ONLY.test(v.split(" ")[0] ?? "") || WEEKDAY_TS.test(v) || ISO_TS.test(v))) {
      out.push(timeRole(v));
      continue;
    }
    if (LEVEL.test(v) || MODULE_LEVEL.test(v)) {
      out.push({ role: byName("level") ?? "text", tsFormat: DEFAULT_TS });
      continue;
    }
    if (/^\d+#\d+:?$/.test(v) || /^pid \d+/.test(v)) {
      out.push({ role: byName("pid_tid") ?? byName("source") ?? "text", tsFormat: DEFAULT_TS });
      continue;
    }
    if (/^\*\d+$/.test(v)) {
      out.push({ role: byName("connection") ?? byName("source") ?? "text", tsFormat: DEFAULT_TS });
      continue;
    }
    if (/^client \S+/.test(v)) {
      out.push({ role: byName("client") ?? byName("source") ?? "text", tsFormat: DEFAULT_TS });
      continue;
    }
    if (/^[a-z_]+\.c\(\d+\)$/i.test(v) && byName("source_file")) {
      out.push({ role: byName("source_file") ?? "text", tsFormat: DEFAULT_TS });
      continue;
    }
    if (/^\(\d+\)/.test(v) && byName("os_error")) {
      out.push({ role: byName("os_error") ?? "text", tsFormat: DEFAULT_TS });
      continue;
    }
    if (/^AH\d{5}:$/.test(v) && byName("error_code")) {
      out.push({ role: byName("error_code") ?? "text", tsFormat: DEFAULT_TS });
      continue;
    }
    out.push({ role: rest, tsFormat: DEFAULT_TS });
    inMessage = true;
  }
  return out;
}

/** 값 모양으로 라벨을 추정한다. 프리셋 판별이 안 됐을 때의 출발점이다. */
export function guessRoles(pieces: Piece[], vocab: RoleDef[]): RoleAssign[] {
  const used = new Set<string>();
  const take = (kind: FieldKind): RoleAssign => {
    used.add(kind.kind);
    return { role: roleForKind(vocab, kind), tsFormat: kind.kind === "timestamp" ? kind.format : DEFAULT_TS };
  };
  return pieces.map((p) => {
    const v = p.value;
    if (v === "" || v === "-") return { role: "ignore", tsFormat: DEFAULT_TS };
    if (!used.has("timestamp") && CLF_TS.test(v)) return take({ kind: "timestamp", format: { kind: "clf" } });
    if (!used.has("timestamp") && ISO_TS.test(v)) return take({ kind: "timestamp", format: { kind: "iso8601" } });
    if (!used.has("client_ip") && (IPV4.test(v) || IPV6.test(v))) return take({ kind: "client_ip" });
    if (!used.has("request_line") && !used.has("method") && /^[A-Z]{3,7} \S+( HTTP\/\d(\.\d)?)?$/.test(v)) return take({ kind: "request_line" });
    if (!used.has("method") && METHOD.test(v)) return take({ kind: "method" });
    if (!used.has("request_target") && v.startsWith("/")) return take({ kind: "request_target" });
    if (!used.has("protocol") && /^HTTP\/\d(\.\d)?$/.test(v)) return take({ kind: "protocol" });
    if (/^\d+$/.test(v)) {
      const num = Number(v);
      if (!used.has("status") && num >= 100 && num <= 599 && v.length === 3) return take({ kind: "status" });
      if (!used.has("bytes_sent")) return take({ kind: "bytes_sent" });
      return { role: "integer", tsFormat: DEFAULT_TS };
    }
    if (p.capture === "quoted") {
      if (!used.has("referrer") && v.startsWith("http")) return take({ kind: "referrer" });
      if (!used.has("user_agent")) return take({ kind: "user_agent" });
    }
    return { role: "text", tsFormat: DEFAULT_TS };
  });
}

function escapeRegex(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function captureFor(piece: Piece, sepChar: string | null): Capture {
  if (sepChar !== null) return { kind: "pattern", pattern: `[^${escapeRegex(sepChar)}]*` };
  if (piece.capture === "pattern") return { kind: "pattern", pattern: "\\S+ \\S+" };
  return { kind: piece.capture } as Capture;
}

/** 무시 조각: 이름 없는 정규식 블록이라 저장되지 않는다. */
function ignorePattern(piece: Piece, sepChar: string | null): string {
  if (sepChar !== null) return `[^${escapeRegex(sepChar)}]*`;
  switch (piece.capture) {
    case "quoted":
      return '"(?:[^"\\\\]|\\\\.)*"';
    case "bracketed":
      return "\\[[^\\]]*\\]";
    case "pattern":
      return "\\S+ \\S+";
    default:
      return "\\S+";
  }
}

function sanitizeName(id: string): string {
  const s = id.replace(/[^A-Za-z0-9_-]/g, "_").replace(/^[^A-Za-z_]+/, "");
  return s === "" ? "field" : s;
}

/** "줄 끝까지" 라벨이 붙은 첫 조각의 위치. 없으면 null. */
export function restIndex(roles: RoleAssign[], vocab: RoleDef[]): number | null {
  const i = roles.findIndex((r) => roleDef(vocab, r.role)?.rest);
  return i === -1 ? null : i;
}

/** 조각과 라벨로 블록 정의를 만든다. 이름은 어휘의 저장 이름을 쓰고 겹치면 번호를 붙인다. */
export function buildBlocks(pieces: Piece[], roles: RoleAssign[], sep: Separator, vocab: RoleDef[]): Block[] {
  const sepChar = sep === "space" ? null : (SEPARATORS.find((s) => s.id === sep)?.char ?? sep);
  const blocks: Block[] = [];
  const names = new Set<string>();
  const restAt = restIndex(roles, vocab);
  pieces.forEach((piece, i) => {
    if (restAt !== null && i > restAt) return;
    if (i > 0) blocks.push(sepChar === null ? { block: "whitespace" } : { block: "literal", text: sepChar });
    const a = roles[i] ?? { role: "ignore", tsFormat: DEFAULT_TS };
    const def = roleDef(vocab, a.role);
    const kind = def?.kind ?? "text";
    if (kind === "ignore") {
      blocks.push({ block: "regex", pattern: ignorePattern(piece, sepChar) });
      return;
    }
    if (def?.rest) {
      if (def.tail && def.tail.length > 0) {
        // 메시지는 게으르게(.*?) 잡고, 뒤에 붙는 항목은 있을 때만 맞는 선택 그룹으로 둔다. 전체는 줄 끝(^…$)에 고정된다.
        blocks.push({ block: "field", name: def.name ?? "message", kind: { kind: "text" }, capture: { kind: "pattern", pattern: ".*?" }, missing: [] });
        for (const t of def.tail) {
          const inner: Block[] = [
            { block: "literal", text: t.quoted ? `, ${t.key}: "` : `, ${t.key}: ` },
            { block: "field", name: t.name, kind: { kind: t.kind } as FieldKind, capture: { kind: "pattern", pattern: t.quoted ? '[^"]*' : "[^,]+" }, missing: ["-"] },
          ];
          if (t.quoted) inner.push({ block: "literal", text: '"' });
          blocks.push({ block: "optional_group", blocks: inner });
        }
      } else {
        blocks.push({ block: "field", name: def.name ?? "message", kind: { kind: "text" }, capture: { kind: "pattern", pattern: ".*" }, missing: [] });
      }
      return;
    }
    const base = def?.name ?? (kind === "text" || kind === "integer" ? `field_${i + 1}` : sanitizeName(a.role));
    let name = base;
    let n = 2;
    while (names.has(name)) name = `${base}_${n++}`;
    names.add(name);
    const fieldKind: FieldKind = kind === "timestamp" ? { kind: "timestamp", format: def?.ts ?? a.tsFormat } : ({ kind } as FieldKind);
    blocks.push({
      block: "field",
      name,
      kind: fieldKind,
      capture: captureFor(piece, sepChar),
      missing: sepChar === null ? ["-"] : ["-", ""],
    });
  });
  return blocks;
}

/** 시작 정의의 시간대·서버 힌트를 물려받아 편집 정의를 만든다. 에러 로그는 시각에 오프셋이 없으므로 한국 시간(UTC+9)으로 간주한다. */
export function buildProfile(base: FormatProfile | null, server: ServerHint, pieces: Piece[], roles: RoleAssign[], sep: Separator, vocab: RoleDef[], kind: LogKind = "access"): FormatProfile {
  const baseName = base?.name ?? (kind === "error" ? "error_log" : "custom");
  const name = baseName.endsWith("_edit") ? baseName : `${baseName}_edit`;
  return {
    schema_version: base?.schema_version ?? 1,
    name,
    version: base?.version ?? 1,
    server_hint: base?.server_hint ?? server,
    timezone: base?.timezone ?? (kind === "error" ? { kind: "fixed", offset_seconds: 9 * 3600 } : { kind: "from_input" }),
    strategy: { kind: "blocks", blocks: buildBlocks(pieces, roles, sep, vocab) },
  };
}

/** 서버 종류에 맞는 템플릿만 고른다. 사용자 프리셋은 항상 보인다. `keep`은 현재 선택이라 목록에 남긴다. */
export function templatesFor<T extends { name: string; source: "builtin" | "user"; profile: FormatProfile }>(server: ServerHint, list: T[], keep: string): T[] {
  return list.filter((p) => {
    if (p.source === "user" || p.name === keep || server === "unknown") return true;
    const hint = p.profile.server_hint;
    if (server === "iis") return hint === "iis";
    return hint === server || hint === "unknown";
  });
}

/** "메시지 + 상세 분리" 뒤에 흡수된 조각에 붙일 표시용 라벨. `key:` 조각에는 항목 이름, 그 값 조각에는 같은 색만. */
export function tailLabels(pieces: Piece[], restAt: number, tail: TailItem[]): Map<number, { label: string; kind: FieldKind["kind"] }> {
  const out = new Map<number, { label: string; kind: FieldKind["kind"] }>();
  let current: TailItem | null = null;
  for (let i = restAt + 1; i < pieces.length; i += 1) {
    const text = pieces[i].text;
    const item = tail.find((t) => text === `${t.key}:`);
    if (item) {
      current = item;
      out.set(i, { label: item.label, kind: item.kind });
    } else if (current) {
      out.set(i, { label: "", kind: current.kind });
    }
  }
  return out;
}
