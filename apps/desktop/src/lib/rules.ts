// 분석 룰: YARA풍 텍스트가 원본이다. 기본 룰은 내장 원문, 사용자 룰은 저장된 뷰(rule_source)로 둔다.
import { emptyFilter, type CondField, type LogFilter, type LogKind, type SavedView } from "../types";
import { describeExpr, parseRule } from "./yara";
import type { FilterExpr } from "../types";

export interface Rule {
  /** "builtin:<id>" 또는 "view:<view_id>". */
  id: string;
  name: string;
  description: string;
  /** 룰 원문. 사용자 룰은 편집할 때 이 원문을 연다. */
  source: string;
  expr: FilterExpr;
  builtin: boolean;
  viewId?: number;
  /** 원문을 파싱하지 못한 사용자 룰의 오류. 이 경우 저장된 조건 객체로 대신 조회한다. */
  error?: string;
  /** 원문이 없거나 깨진 옛 저장 뷰의 조건 객체. */
  legacyFilter?: LogFilter;
}

/** 기본 룰 원문. 편집기의 예시이기도 하다. 서명은 RE2(DuckDB)와 JS 정규식 양쪽에서 같은 뜻이어야 한다. */
export const BUILTIN_SOURCES: string[] = [
  `rule all_requests
{
    meta:
        name = "전체 요청"
        description = "조건 없이 전체를 봅니다."
    condition:
        true
}`,
  `rule ok_responses
{
    meta:
        name = "정상 응답"
        description = "상태 2xx 응답."
    condition:
        status in 200..299
}`,
  `rule sql_injection
{
    meta:
        name = "SQL Injection 흔적"
        description = "경로·쿼리에 UNION SELECT, ' OR 1=1, sleep(), information_schema, %27 같은 서명."
    strings:
        $union  = /union[%20\\s+]+(all[%20\\s+]+)?select/ nocase
        $quote  = /'\\s*(or|and)\\s*'?\\d/ nocase
        $tauto  = /\\b(or|and)\\s+\\d+\\s*=\\s*\\d+/ nocase
        $func   = /sleep\\(\\d|benchmark\\(|load_file\\(|xp_cmdshell|information_schema/ nocase
        $enc    = "%27"
        $cmt    = /--\\s|;\\s*--|\\/\\*.*\\*\\//
    condition:
        any of them
}`,
  `rule xss
{
    meta:
        name = "XSS 흔적"
        description = "<script, javascript:, onerror=, alert( 같은 서명."
    strings:
        $tag  = /<script|%3cscript|%3c%2fscript/ nocase
        $js   = "javascript:" nocase
        $evt  = /on(error|load)\\s*=/ nocase
        $call = /alert\\(|document\\.cookie/ nocase
    condition:
        any of them
}`,
  `rule path_traversal
{
    meta:
        name = "경로 탐색 시도"
        description = "../ 와 그 인코딩 변형."
    strings:
        $dots = /\\.\\.\\/|\\.\\.\\\\|%2e%2e%2f|%2e%2e\\/|\\.\\.%2f|%252e%252e/ nocase
    condition:
        $dots
}`,
  `rule command_injection
{
    meta:
        name = "명령 주입 흔적"
        description = "; cat, | wget, $( , /bin/sh 같은 서명."
    strings:
        $semi  = /;\\s*(cat|ls|id|whoami|wget|curl|nc|bash|sh)\\b/ nocase
        $pipe  = /\\|\\s*(cat|ls|id|whoami|wget|curl)\\b/ nocase
        $subst = /%60|\\$\\(|%7c%20/
        $shell = /\\/bin\\/(ba)?sh/
    condition:
        any of them
}`,
  `rule scanner_paths
{
    meta:
        name = "취약점 스캐너 경로"
        description = "wp-login, phpmyadmin, .env, .git, /etc/passwd 등 스캐너가 두드리는 경로."
    strings:
        $wp    = /\\/wp-login\\.php|\\/wp-admin|\\/xmlrpc\\.php/ nocase
        $admin = /\\/phpmyadmin|\\/manager\\/html|\\/actuator\\//i
        $files = /\\/\\.env|\\/\\.git\\/|\\/\\.aws\\/|\\/etc\\/passwd|\\/config\\.php|\\/shell\\.php|\\/cgi-bin\\// nocase
    condition:
        any of them
}`,
  `rule sql_injection_success
{
    meta:
        name = "SQL Injection · 성공 응답"
        description = "서명이 있는데 2xx로 응답한 요청. 우선 확인 대상."
    strings:
        $union = /union[%20\\s+]+(all[%20\\s+]+)?select/ nocase
        $quote = /'\\s*(or|and)\\s*'?\\d/ nocase
        $enc   = "%27"
    condition:
        status in 200..299 and any of them
}`,
  `rule sqlmap
{
    meta:
        name = "sqlmap 흔적"
        description = "sqlmap 도구 패턴. UA가 없어도 0x71…71/CHR(113) 경계 문자열, 전용 함수, 4자리 항진식, UNION ALL SELECT NULL로 잡습니다."
    strings:
        // 결과를 감싸는 경계 문자열: CONCAT(0x716b6a7671, …, 0x7176767a71) — 0x71은 'q'
        $hexmark  = /0x71[0-9a-f]{4,}71/ nocase
        $chrmark  = /chr(?:\\(|%28)113(?:\\)|%29)(?:\\|\\||%7c%7c)/ nocase
        $charmark = /char(?:\\(|%28)113(?:\\)|%29)(?:\\+|%2b)/ nocase
        // sqlmap이 즐겨 쓰는 함수·구문(공백은 %20, + 로 인코딩될 수 있음)
        $funcs    = /(?:extractvalue|updatexml|gtid_subset|json_keys|make_set|elt|benchmark|pg_sleep)(?:\\(|%28)|procedure(?:\\s|%20|\\+)+analyse|waitfor(?:\\s|%20|\\+)+delay|sleep(?:\\(|%28)\\d/ nocase
        $taut     = /(?:^|%20|\\+|[^a-z0-9_])(?:and|or)(?:\\s|%20|\\+)+\\d{4}(?:=|%3d)\\d{4}/ nocase
        $union    = /union(?:\\s|%20|\\+)+all(?:\\s|%20|\\+)+select(?:\\s|%20|\\+)+null/ nocase
        $casewhen = /select(?:\\s|%20|\\+)+(?:\\(|%28)case(?:\\s|%20|\\+)+when/ nocase
        $ua       = /sqlmap/ nocase
    condition:
        ua contains $ua or any of ($hexmark, $chrmark, $charmark, $funcs, $taut, $union, $casewhen)
}`,
  `rule scanner_tools
{
    meta:
        name = "스캐너 도구 UA"
        description = "nikto, nuclei, masscan, zgrab, nmap, dirbuster, gobuster, ffuf, acunetix, nessus, wpscan 같은 취약점 스캐너의 User-Agent."
    strings:
        $tools = /nikto|nuclei|masscan|zgrab|nmap|dirbuster|gobuster|ffuf|feroxbuster|acunetix|nessus|wpscan|whatweb|openvas|burp|arachni/ nocase
    condition:
        ua contains $tools
}`,
  `rule log4shell
{
    meta:
        name = "Log4Shell(JNDI) 시도"
        description = "\${jndi:ldap://…} 페이로드와 그 인코딩·난독화 변형."
    strings:
        $plain = /\\$\\{jndi:/ nocase
        $enc   = /%24%7bjndi/ nocase
        $obf   = /\\$\\{(?:lower|upper|env|sys|::-)[^}]*\\}(?:j|\\$)/ nocase
    condition:
        any of them
}`,
  `rule lfi_ssrf
{
    meta:
        name = "파일 포함·SSRF 시도"
        description = "php://, file://, data:, /proc/self, /etc/shadow, 그리고 url= 파라미터로 내부 주소를 넘기는 시도."
    strings:
        $wrap  = /(?:php|file|data|expect|zip|phar):(?:\\/\\/|%2f%2f)/ nocase
        $proc  = /\\/proc\\/self|\\/etc\\/shadow|win\\.ini|boot\\.ini/ nocase
        $ssrf  = /[?&](?:url|uri|target|dest|redirect|next|src|feed)=(?:https?(?::|%3a)(?:\\/\\/|%2f%2f))?(?:127\\.0\\.0\\.1|localhost|169\\.254\\.169\\.254|10\\.\\d+\\.\\d+\\.\\d+|192\\.168\\.)/ nocase
    condition:
        any of them
}`,
  `rule webshell_upload
{
    meta:
        name = "웹셸 업로드·실행 시도"
        description = "업로드 경로 아래의 실행 파일, cmd=/exec= 파라미터, eval·base64_decode·system 호출."
    strings:
        $upload = /\\/(?:upload|uploads|files|attach|attachments|tmp|temp)\\/[^?]*\\.(?:php\\d?|phtml|jsp|jspx|asp|aspx|cgi)/ nocase
        $param  = /[?&](?:cmd|exec|command|execute|shell)=/ nocase
        $calls  = /(?:eval|assert|system|passthru|shell_exec|base64_decode)(?:\\(|%28)/ nocase
    condition:
        any of them
}`,
  `rule bots
{
    meta:
        name = "봇·스크립트 UA"
        description = "python, curl, wget, go-http, scrapy 같은 자동화 도구의 User-Agent."
    strings:
        $tools = /python|curl|wget|go-http-client|scrapy|libwww|httpclient|okhttp|java\\// nocase
    condition:
        ua contains $tools or ua is null
}`,
];

/**
 * 에러 로그 기본 룰 원문. 문구는 nginx·apache 에러 로그에 실제로 찍히는 문장을 그대로 쓴다.
 * `$id`만 쓴 서명은 메시지에 적용된다(접근 룰은 경로).
 */
export const BUILTIN_ERROR_SOURCES: string[] = [
  `rule all_errors
{
    meta:
        name = "전체 에러"
        description = "조건 없이 에러 로그 전체를 봅니다."
    condition:
        true
}`,
  `rule severe_levels
{
    meta:
        name = "심각(crit·alert·emerg)"
        description = "레벨이 crit, alert, emerg인 줄. apache는 core:crit 처럼 모듈 이름이 앞에 붙어도 잡습니다."
    strings:
        $sev = /crit|alert|emerg/ nocase
    condition:
        level matches $sev
}`,
  `rule error_level
{
    meta:
        name = "오류(error)"
        description = "레벨이 error인 줄. nginx [error], apache [core:error] 둘 다 잡습니다."
    strings:
        $lv = /error/ nocase
    condition:
        level matches $lv
}`,
  `rule warn_level
{
    meta:
        name = "경고(warn)"
        description = "레벨이 warn인 줄. nginx [warn], apache [php:warn]."
    strings:
        $lv = /warn/ nocase
    condition:
        level matches $lv
}`,
  `rule file_missing
{
    meta:
        name = "파일 없음"
        description = "nginx open()/stat() failed (2: No such file or directory), apache AH00035 File does not exist."
    strings:
        $nginx  = "No such file or directory" nocase
        $stat   = "stat() failed" nocase
        $apache = "File does not exist" nocase
    condition:
        any of them
}`,
  `rule permission_denied
{
    meta:
        name = "권한 거부"
        description = "nginx failed (13: Permission denied), apache AH01630 client denied by server configuration."
    strings:
        $perm   = "Permission denied" nocase
        $denied = "denied by server configuration" nocase
    condition:
        any of them
}`,
  `rule upstream_failed
{
    meta:
        name = "업스트림 연결 실패"
        description = "connect() failed (111: Connection refused), upstream timed out, no live upstreams, upstream prematurely closed connection, upstream sent too big header."
    strings:
        $refused = "Connection refused" nocase
        $timeout = "upstream timed out" nocase
        $nolive  = "no live upstreams" nocase
        $closed  = "upstream prematurely closed connection" nocase
        $reset   = "Connection reset by peer" nocase
        $hdr     = "upstream sent too big header" nocase
    condition:
        any of them
}`,
  `rule body_too_large
{
    meta:
        name = "요청 본문 초과"
        description = "client intended to send too large body. nginx client_max_body_size, apache Request Entity Too Large."
    strings:
        $big = "client intended to send too large body" nocase
        $ent = "Request Entity Too Large" nocase
    condition:
        any of them
}`,
  `rule ssl_handshake
{
    meta:
        name = "SSL 핸드쉐이크 오류"
        description = "SSL_do_handshake() failed, SSL routines, SSL handshake error, SSL_read() failed."
    strings:
        $hs   = "SSL_do_handshake" nocase
        $rt   = "SSL routines" nocase
        $err  = "SSL handshake error" nocase
        $read = "SSL_read() failed" nocase
    condition:
        any of them
}`,
  `rule php_fastcgi
{
    meta:
        name = "PHP·FastCGI 실패"
        description = "FastCGI sent in stderr, PHP message, PHP Fatal error."
    strings:
        $fcgi  = "FastCGI sent in stderr" nocase
        $php   = "PHP message" nocase
        $fatal = "PHP Fatal error" nocase
    condition:
        any of them
}`,
  `rule directory_index_forbidden
{
    meta:
        name = "디렉터리 색인 금지"
        description = "nginx directory index of ... is forbidden, apache AH01276 Directory index forbidden by Options directive."
    strings:
        $nginx  = "directory index of" nocase
        $apache = "Directory index forbidden" nocase
    condition:
        any of them
}`,
];

/** `$id`만 쓴 서명이 적용될 필드. 접근 로그는 경로, 에러 로그는 메시지. */
export function defaultRuleField(kind: LogKind): CondField {
  return kind === "error" ? "message" : "request_target";
}

function compileBuiltin(src: string, kind: LogKind): Rule {
  const r = parseRule(src, { defaultField: defaultRuleField(kind) });
  if (!r.ok) throw new Error(`기본 룰 파싱 실패: ${r.errors[0].message}\n${src}`);
  return { id: `builtin:${r.rule.id}`, name: r.rule.name, description: r.rule.description, source: src, expr: r.rule.expr, builtin: true };
}

/** 룰 목록 맨 위의 북마크 뷰. 조건식이 아니라 북마크 표에 있는 행만 보인다. */
export const BOOKMARK_RULE: Rule = {
  id: "builtin:bookmarks",
  name: "북마크",
  description: "북마크한 행만 봅니다. 행 앞의 별을 누르면 북마크됩니다.",
  source: "",
  expr: { kind: "true" },
  builtin: true,
  legacyFilter: { ...emptyFilter(), bookmarked_only: true },
};

export const BUILTIN_RULES: Rule[] = [BOOKMARK_RULE, ...BUILTIN_SOURCES.map((s) => compileBuiltin(s, "access"))];

export const BUILTIN_ERROR_RULES: Rule[] = [BOOKMARK_RULE, ...BUILTIN_ERROR_SOURCES.map((s) => compileBuiltin(s, "error"))];

/** 로그 종류별 기본 룰. 북마크 룰은 두 종류가 함께 쓴다. */
export function builtinRules(kind: LogKind): Rule[] {
  return kind === "error" ? BUILTIN_ERROR_RULES : BUILTIN_RULES;
}

/** 사이드바에 보일 목록: 그 종류의 기본 룰 + 그 종류로 저장된 사용자 룰(종류를 적지 않은 옛 뷰는 접근 로그). */
export function rulesFor(kind: LogKind, views: SavedView[]): Rule[] {
  return [...builtinRules(kind), ...views.filter((v) => (v.definition.filter.log_kind ?? "access") === kind).map(ruleFromView)];
}

/** 룰을 실제 조회 조건으로. 시간·활성 같은 화면 조건은 base로 얹는다. */
export function ruleFilter(rule: Rule, base: Partial<LogFilter> = {}): LogFilter {
  if (rule.legacyFilter) return { ...rule.legacyFilter, ...base };
  return { ...emptyFilter(), ...base, expr: rule.expr };
}

/** 저장된 뷰를 사용자 룰로. 원문이 있으면 다시 파싱하고, 없거나 깨졌으면 저장된 조건 객체를 쓴다. */
export function ruleFromView(v: SavedView): Rule {
  const src = v.definition.rule_source;
  if (src) {
    const r = parseRule(src, { defaultField: defaultRuleField(v.definition.filter.log_kind ?? "access") });
    if (r.ok) return { id: `view:${v.view_id}`, name: v.name, description: r.rule.description, source: src, expr: r.rule.expr, builtin: false, viewId: v.view_id };
    return {
      id: `view:${v.view_id}`,
      name: v.name,
      description: `원문 오류 ${r.errors[0].line}:${r.errors[0].col} ${r.errors[0].message}`,
      source: src,
      expr: v.definition.filter.expr ?? { kind: "true" },
      builtin: false,
      viewId: v.view_id,
      error: r.errors[0].message,
      legacyFilter: v.definition.filter,
    };
  }
  const f = v.definition.filter;
  return {
    id: `view:${v.view_id}`,
    name: v.name,
    description: f.expr ? describeExpr(f.expr) : describeFilter(f),
    source: "",
    expr: f.expr ?? { kind: "true" },
    builtin: false,
    viewId: v.view_id,
    legacyFilter: f,
  };
}

/** 단순 조건 객체를 한 줄 문장으로(원문 없는 옛 뷰용). */
export function describeFilter(f: LogFilter): string {
  const parts: string[] = [];
  if (f.status !== null) parts.push(`상태 ${f.status}`);
  else if (f.status_class !== null) parts.push(`상태 ${f.status_class}xx`);
  if (f.method) parts.push(`메서드 ${f.method}`);
  if (f.client_ip) parts.push(`IP ${f.client_ip}`);
  if (f.target_contains) parts.push(`경로에 "${f.target_contains}" 포함`);
  if (f.target_regex) parts.push("경로 정규식");
  if (f.expr) parts.push(describeExpr(f.expr));
  if (f.time_from_micros !== null || f.time_to_micros !== null) parts.push("시간 범위");
  if (f.job_id !== null) parts.push(`작업 ${f.job_id}`);
  return parts.length === 0 ? "조건 없음" : parts.join(" · ");
}
