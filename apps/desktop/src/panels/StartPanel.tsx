// 시작 화면: 서버 종류 → 폴더 선택 → 로그 파일 목록 → 첫 줄 자동 판별 → "이 포맷이 맞나요?" → 파싱 시작.
import { useEffect, useMemo, useRef, useState, type DragEvent, type ReactNode } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText } from "../api";
import { useAppState, type ImportItem, type LogKind } from "../state";
import { CasesPanel } from "./CasesPanel";
import { formatBytes, formatCount } from "../lib/format";
import { baseName, compareNatural, relativeTo } from "../lib/paths";
import {
  buildProfile,
  guessErrorRoles,
  guessRoles,
  guessSeparator,
  inferTsFormat,
  restIndex,
  roleDef,
  rolesFromProfile,
  separatorOf,
  tailLabels,
  tokenize,
  vocabFor,
  type Piece,
  type Role,
  type RoleAssign,
  type RoleDef,
  type Separator,
} from "../lib/puzzle";
import type { FormatProfile, LineOutcome, PreviewResult, ProfileSpec, ProfileView, ScannedFile, ServerHint } from "../types";

const SERVERS: { id: ServerHint; label: string }[] = [
  { id: "apache", label: "Apache" },
  { id: "nginx", label: "Nginx" },
  { id: "iis", label: "IIS" },
  { id: "unknown", label: "모름" },
];

/** 판별용으로 읽는 선두 줄 수. IIS처럼 지시문이 앞에 오는 파일에서도 첫 레코드를 찾도록 한 줄보다 넉넉히 둔다. */
const SAMPLE_LINES = 8;

/** 서버 힌트·로그 종류에 맞는 기본 포함 패턴. 후보 선정용이며 포맷은 내용으로 판별한다. */
export function defaultInclude(server: ServerHint, kind: LogKind = "access"): string {
  if (kind === "error") {
    switch (server) {
      case "iis":
        return "httperr*.log";
      case "apache":
        return "error*.log*, error_log*";
      case "nginx":
        return "error*.log*";
      default:
        return "*error*.log*, error_log*";
    }
  }
  switch (server) {
    case "iis":
      return "u_ex*.log, *.log";
    case "apache":
    case "nginx":
      return "access*.log*, access_log*";
    default:
      return "*.log*";
  }
}

const KINDS: { id: LogKind; label: string }[] = [
  { id: "access", label: "Access Log" },
  { id: "error", label: "Error Log" },
];

export function splitPatterns(text: string): string[] {
  return text
    .split(/[,\n]/)
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
}

/** 판별된 프로필 중 가장 많은 파일이 쓰는 것. 없으면 null. */
export function majorityProfile(files: ScannedFile[]): string | null {
  const counts = new Map<string, number>();
  for (const f of files) {
    if (f.best_profile) counts.set(f.best_profile, (counts.get(f.best_profile) ?? 0) + 1);
  }
  let best: string | null = null;
  let max = 0;
  for (const [name, n] of counts) {
    if (n > max) {
      best = name;
      max = n;
    }
  }
  return best;
}

interface Sample {
  path: string;
  /** 보여줄 줄의 원문. UTF-8이 아니거나 너무 길면 null. */
  raw: string | null;
  /** 그 줄의 파싱 결과(제외 줄은 건너뛰고 첫 레코드나 첫 오류). */
  outcome: LineOutcome | null;
  /** 정의가 아직 없으면(에러 로그 첫 진입) null. */
  preview: PreviewResult | null;
}

export function StartPanel() {
  const { selection, setSelection, selections, setSelections, setStage, setNotice, createCase, progress, clearFinished, startImports, logKind, setLogKind } = useAppState();
  const [includes, setIncludes] = useState<Record<LogKind, string>>({ access: defaultInclude("apache", "access"), error: defaultInclude("apache", "error") });
  const isError = logKind === "error";
  const [exclude, setExclude] = useState("*.bak, *.tmp");
  const [recursive, setRecursive] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [profiles, setProfiles] = useState<ProfileView[]>([]);
  const [sample, setSample] = useState<Sample | null>(null);
  const [sampling, setSampling] = useState(false);
  const [starting, setStarting] = useState(false);
  const sampleReq = useRef(0);
  // 퍼즐: 구분자, 조각별 라벨, 사용자 변경 여부. 마지막으로 이 화면이 만든 정의(JSON)는 다시 라벨로 되돌리지 않는다.
  const [sep, setSep] = useState<Separator>("space");
  const [roles, setRoles] = useState<RoleAssign[]>([]);
  const [dirty, setDirty] = useState(false);
  const lastBuilt = useRef<string | null>(null);

  useEffect(() => {
    api
      .listProfiles()
      .then((l) => setProfiles(l.profiles))
      .catch((e) => setNotice(errorText(e)));
  }, [setNotice]);

  /** 한 폴더를 접근 로그·에러 로그 패턴으로 각각 탐색한다. 접근 로그만 프리셋으로 판별한다. */
  const runScan = async (root: string, server: ServerHint, inc: Record<LogKind, string>) => {
    setScanning(true);
    try {
      const req = (kind: LogKind) =>
        api.scanFiles({
          root,
          recursive,
          max_depth: null,
          include: splitPatterns(inc[kind]),
          exclude: splitPatterns(exclude),
          detect: kind === "access",
          sample_lines: 200,
        });
      const [ra, re] = await Promise.all([req("access"), req("error")]);
      // 같은 파일이 두 패턴에 다 걸리면 접근 로그 쪽에만 둔다.
      const accessPaths = new Set(ra.files.map((f) => f.path));
      const errorFiles = re.files.filter((f) => !accessPaths.has(f.path));
      const detected = ra.files.filter((f) => f.best_profile);
      const best = majorityProfile(detected);
      setSelections({
        access: { ...selections.access, server, root, scan: ra, files: detected.length > 0 ? detected : ra.files, profileName: best ?? selections.access.profileName, profile: null },
        error: { ...selections.error, server, root, scan: { ...re, files: errorFiles }, files: errorFiles, profile: null },
      });
      const hasAccess = ra.files.length > 0;
      const hasError = errorFiles.length > 0;
      if (!hasAccess && !hasError) {
        setNotice("이 폴더에서 로그 파일을 찾지 못했습니다. 서버 종류를 바꾸거나 파일명 패턴을 확인하세요.");
      } else {
        const errs = ra.errors.length + re.errors.length;
        setNotice(errs > 0 ? `읽지 못한 항목 ${errs}개가 있습니다. 파일 목록 아래에서 확인하세요.` : null);
      }
      if (!(logKind === "access" ? hasAccess : hasError)) setLogKind(hasAccess ? "access" : "error");
    } catch (e) {
      setNotice(errorText(e));
    } finally {
      setScanning(false);
    }
  };

  /** 탐색 결과와 선택을 비우고 폴더 선택 화면으로 돌아간다. 서버 종류는 유지한다. */
  const goHome = () => {
    const reset = (sel: typeof selection) => ({ ...sel, root: "", scan: null, files: [], profile: null });
    setSelections({ access: reset(selections.access), error: reset(selections.error) });
    setSample(null);
    setNotice(null);
  };

  const pickFolder = async () => {
    try {
      const p = await open({ directory: true, multiple: false });
      if (typeof p === "string" && p) await runScan(p, selection.server, includes);
    } catch (e) {
      setNotice(errorText(e));
    }
  };

  const chooseServer = async (s: ServerHint) => {
    const inc: Record<LogKind, string> = { access: defaultInclude(s, "access"), error: defaultInclude(s, "error") };
    setIncludes(inc);
    if (selection.root) await runScan(selection.root, s, inc);
    else setSelections({ access: { ...selections.access, server: s }, error: { ...selections.error, server: s } });
  };

  const toggle = (path: string) => {
    const has = selection.files.some((f) => f.path === path);
    const files = has ? selection.files.filter((f) => f.path !== path) : [...selection.files, ...(selection.scan?.files.filter((f) => f.path === path) ?? [])];
    setSelection({ ...selection, files });
  };

  // 접근 로그는 판별된 프리셋으로 바로 미리보기한다. 에러 로그는 라벨로 만든 정의가 생긴 뒤에만 미리보기한다.
  const spec: ProfileSpec | null = selection.profile
    ? { kind: "definition", profile: selection.profile }
    : isError
      ? null
      : { kind: "preset", name: selection.profileName };

  // 판별 대상 파일: 선택한 프로필로 판별된 첫 파일, 없으면 첫 선택 파일.
  const target = selection.files.find((f) => f.best_profile === selection.profileName) ?? selection.files[0] ?? null;
  const targetPath = target?.path ?? null;
  // 탐색을 다시 하면 같은 파일이라도 샘플을 다시 읽는다(서버 종류 변경 등).
  const scanId = selection.scan;
  // 정의 내용이 바뀔 때마다 다시 판별해야 하므로 내용 자체를 키로 쓴다(정의는 작다).
  const specKey = selection.profile ? `def:${JSON.stringify(selection.profile)}` : isError ? "none" : `preset:${selection.profileName}`;

  // 대상 파일이나 프로필이 바뀌면 첫 줄을 다시 읽어 판별한다. 늦게 온 응답은 버린다.
  useEffect(() => {
    if (!targetPath) {
      setSample(null);
      return;
    }
    const id = ++sampleReq.current;
    // 다른 파일(다른 탭 포함)로 바뀌면 이전 줄로 라벨을 추정하지 않도록 먼저 비운다.
    setSample((prev) => (prev && prev.path !== targetPath ? null : prev));
    setSampling(true);
    Promise.all([api.sampleLines(targetPath, SAMPLE_LINES), spec ? api.previewFormat(targetPath, spec, SAMPLE_LINES) : Promise.resolve(null)])
      .then(([lines, preview]) => {
        if (id !== sampleReq.current) return;
        const outcome = preview ? (preview.outcomes.find((o) => o.kind !== "skipped") ?? preview.outcomes[0] ?? null) : null;
        const firstText = lines.lines.find((l) => l.text !== null && l.text.trim() !== "");
        const raw = outcome ? (lines.lines.find((l) => l.line_number === outcome.line_number)?.text ?? null) : (firstText?.text ?? null);
        setSample({ path: targetPath, raw, outcome, preview });
      })
      .catch((e) => {
        if (id === sampleReq.current) setNotice(errorText(e));
      })
      .finally(() => {
        if (id === sampleReq.current) setSampling(false);
      });
    // spec 객체는 매 렌더 새로 만들어지므로 내용 키로 비교한다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [targetPath, specKey, scanId, setNotice]);

  // ----- 퍼즐 매핑 -----
  const raw = sample?.raw ?? null;
  const baseProfile: FormatProfile | null = selection.profile ?? (isError ? null : (profiles.find((p) => p.name === selection.profileName)?.profile ?? null));
  const baseKey = baseProfile ? JSON.stringify(baseProfile) : "none";
  const isW3c = baseProfile?.strategy.kind === "w3c";
  const vocab = vocabFor(selection.server, logKind);
  const pieces = useMemo<Piece[]>(() => (raw ? tokenize(raw, sep) : []), [raw, sep]);

  // 샘플 줄이나 시작 정의가 바뀌면 라벨을 새로 깐다. 이 화면이 방금 만든 정의면 건너뛴다(라벨이 원본).
  useEffect(() => {
    if (!raw || baseKey === lastBuilt.current) return;
    const s = (baseProfile ? separatorOf(baseProfile) : null) ?? guessSeparator(raw);
    const np = tokenize(raw, s);
    setSep(s);
    setRoles((baseProfile ? rolesFromProfile(baseProfile, np, vocab) : null) ?? (isError ? guessErrorRoles(np, vocab) : guessRoles(np, vocab)));
    // 에러 로그는 프리셋이 없으므로 추정한 라벨로 바로 정의를 만들어 미리보기한다.
    setDirty(isError && !baseProfile);
    // baseProfile은 baseKey로 비교한다. 서버를 바꾸면 어휘가 바뀌므로 라벨도 다시 깐다.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [raw, baseKey, selection.server, logKind]);

  // 사용자가 라벨·구분자를 바꾸면 정의를 만들어 적용한다. 적용되면 위의 판별 효과가 다시 파싱한다.
  useEffect(() => {
    if (!dirty || !raw) return;
    const built = buildProfile(baseProfile, selection.server, pieces, roles, sep, vocab, logKind);
    lastBuilt.current = JSON.stringify(built);
    setSelection({ ...selection, profile: built });
    setDirty(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [dirty]);

  const setRole = (i: number, role: Role) => {
    // 시간 형식은 값 모양으로 추정한다. 다른 형식은 고급 편집에서 고친다.
    setRoles(roles.map((r, k) => (k === i ? { role, tsFormat: role === "timestamp" ? inferTsFormat(pieces[i]?.value ?? "") : r.tsFormat } : r)));
    setDirty(true);
  };
  const swapRoles = (a: number, b: number) => {
    if (a === b) return;
    const next = [...roles];
    [next[a], next[b]] = [next[b], next[a]];
    setRoles(next);
    setDirty(true);
  };
  /** 탭별 파싱 정의. 접근 로그는 판별된 프리셋, 에러 로그는 라벨로 만든 정의만 쓴다. */
  const specFor = (kind: LogKind): ProfileSpec | null => {
    const sel = selections[kind];
    if (sel.profile) return { kind: "definition", profile: sel.profile };
    return kind === "error" ? null : { kind: "preset", name: sel.profileName };
  };

  /** 접근 로그 → 에러 로그 순서로 큐에 넣어 한 번에 파싱한다. */
  const start = async () => {
    const items: ImportItem[] = [];
    const skipped: string[] = [];
    for (const k of KINDS) {
      const sel = selections[k.id];
      if (sel.files.length === 0) continue;
      const sp = specFor(k.id);
      if (!sp) {
        skipped.push(k.label);
        continue;
      }
      items.push({ label: k.label, request: { profile: sp, paths: sel.files.map((f) => f.path), replaces_job_id: null, batch_max_rows: null, batch_max_bytes: null } });
    }
    if (items.length === 0) {
      setNotice(skipped.length > 0 ? `${skipped.join(", ")} 탭을 열어 포맷을 먼저 확인하세요.` : "가져올 파일을 하나 이상 선택하세요.");
      return;
    }
    setStarting(true);
    try {
      // 파싱 한 번 = 케이스 하나. 열린 케이스가 있어도 새 파일을 만들어 같은 로그가 두 번 쌓이지 않게 한다.
      // 저장 위치는 묻지 않는다. 앱 데이터의 cases/ 아래에 폴더 이름과 시각으로 DB를 만든다.
      await createCase(baseName(selection.root));
      clearFinished();
      setStage("importing");
      if (skipped.length > 0) setNotice(`${skipped.join(", ")}는 포맷을 확인하지 않아 건너뜁니다.`);
      await startImports(items);
    } catch (e) {
      setNotice(errorText(e));
      setStage("start");
    } finally {
      setStarting(false);
    }
  };

  const scan = selection.scan;
  const checked = new Set(selection.files.map((f) => f.path));
  const sortedFiles = useMemo(() => [...(scan?.files ?? [])].sort((a, b) => compareNatural(a.path, b.path)), [scan]);
  const running = progress !== null;

  return (
    <section className="start">
      {!scan ? (
        <div className="start-hero">
          <h1>로그 폴더를 선택하세요</h1>
          <p className="lead">폴더 안의 접근 로그와 에러 로그를 찾고 첫 줄을 읽어 포맷을 판별합니다. 원문은 저장하지 않고 파싱된 필드만 프로젝트 파일에 남깁니다.</p>
          <div className="server-tiles" role="radiogroup" aria-label="서버 종류">
            {SERVERS.map((sv) => (
              <button
                key={sv.id}
                role="radio"
                aria-checked={selection.server === sv.id}
                className={`tile ${selection.server === sv.id ? "on" : ""}`}
                onClick={() => chooseServer(sv.id)}
                disabled={scanning}
              >
                <span className="tile-name">{sv.label}</span>
                <span className="tile-hint">{defaultInclude(sv.id, "access")}</span>
                <span className="tile-hint">{defaultInclude(sv.id, "error")}</span>
              </button>
            ))}
          </div>
          <button className="primary big" onClick={pickFolder} disabled={scanning}>
            {scanning ? "탐색 중…" : "폴더 선택"}
          </button>
          <div className="patterns">
            <div className="patterns-title">파일명 패턴</div>
            <PatternForm includes={includes} exclude={exclude} recursive={recursive} setIncludes={setIncludes} setExclude={setExclude} setRecursive={setRecursive} />
          </div>
        </div>
      ) : null}
      {!scan ? (
        <CasesPanel />
      ) : (
        <div className="start-bar">
          <button onClick={goHome} disabled={scanning} title="탐색 결과를 지우고 처음 화면으로" aria-label="홈">
            ⌂ 홈
          </button>
          <div className="seg" role="group" aria-label="서버 종류">
            {SERVERS.map((sv) => (
              <button key={sv.id} className={selection.server === sv.id ? "on" : ""} onClick={() => chooseServer(sv.id)} disabled={scanning}>
                {sv.label}
              </button>
            ))}
          </div>
          <span className="mono ellipsis" title={selection.root}>
            {selection.root}
          </span>
          <button onClick={pickFolder} disabled={scanning}>
            {scanning ? "탐색 중…" : "다른 폴더"}
          </button>
          <span className="bar-sep" aria-hidden="true" />
          <span className="muted small">{KINDS.filter((k) => selections[k.id].files.length > 0).map((k) => `${k.label} ${formatCount(selections[k.id].files.length)}개`).join(" + ") || "선택된 파일 없음"}</span>
          <button className="primary" onClick={start} disabled={starting || running || scanning || KINDS.every((k) => selections[k.id].files.length === 0)} title="결과는 앱 데이터의 cases 폴더에 새 케이스 파일로 저장됩니다.">
            {running ? "다른 파싱이 진행 중" : starting ? "시작하는 중…" : "파싱 시작"}
          </button>
          <details className="advanced">
            <summary>파일명 패턴</summary>
            <PatternForm includes={includes} exclude={exclude} recursive={recursive} setIncludes={setIncludes} setExclude={setExclude} setRecursive={setRecursive}>
              <button onClick={() => selection.root && runScan(selection.root, selection.server, includes)} disabled={scanning || !selection.root}>
                다시 탐색
              </button>
            </PatternForm>
          </details>
        </div>
      )}

      {scan && (
        <div className="tabs kind-tabs" role="tablist" aria-label="로그 종류">
          {KINDS.filter((k) => (selections[k.id].scan?.files.length ?? 0) > 0).map((k) => (
            <button key={k.id} role="tab" aria-selected={logKind === k.id} className={logKind === k.id ? "on" : ""} onClick={() => setLogKind(k.id)}>
              {k.label} <span className="tab-count">{formatCount(selections[k.id].scan?.files.length ?? 0)}</span>
            </button>
          ))}
        </div>
      )}

      {scan && scan.files.length > 0 && (
        <div className="files">
          <div className="files-head">
            <span>
              로그 파일 {formatCount(scan.files.length)}개 · {formatBytes(scan.files.reduce((a, f) => a + f.file_size, 0))}
              {!isError && fileSummary(scan.files, selection.profileName)}
              {scan.truncated && <span className="muted"> · 항목 상한에 걸려 일부만 표시</span>}
            </span>
            <span className="grow" />
            <span className="muted">{formatCount(checked.size)}개 선택</span>
            <button className="linklike" onClick={() => setSelection({ ...selection, files: checked.size === scan.files.length ? [] : scan.files })}>
              {checked.size === scan.files.length ? "모두 해제" : "모두 선택"}
            </button>
          </div>
          <div className="files-grid" role="list">
            {sortedFiles.map((f) => (
              <FileRow key={f.path} f={f} root={selection.root} majority={isError ? null : selection.profileName} checked={checked.has(f.path)} onToggle={() => toggle(f.path)} />
            ))}
          </div>
          {scan.errors.length > 0 && (
            <details className="issues">
              <summary>읽지 못한 항목 {scan.errors.length}개</summary>
              <ul>
                {scan.errors.map((e) => (
                  <li key={e.path}>
                    <span className="mono">{e.path}</span> — {e.message}
                  </li>
                ))}
              </ul>
            </details>
          )}
        </div>
      )}

      {target && (
        <div className="confirm-card" role="region" aria-label="포맷 확인">
          <div className="confirm-head">
            <span className="muted small">
              {relativeTo(selection.root, target.path)}
              {sample?.outcome ? ` ${sample.outcome.line_number}번째 줄` : ""}
              {sampling ? " · 읽는 중…" : ""}
            </span>
            {sample?.preview && <span className={`small ${sample.preview.errors === 0 && sample.preview.records > 0 ? "ok" : "warn"}`}>{matchNote(sample.preview)}</span>}
          </div>
          {sample &&
            (sample.raw === null ? (
              <p className="muted">이 줄은 UTF-8이 아니거나 너무 길어 표시할 수 없습니다.</p>
            ) : isW3c ? (
              <>
                <pre className="raw-line">{sample.raw}</pre>
                <p className="muted small">IIS W3C 로그는 파일 안의 #Fields 헤더로 필드를 자동 구조화합니다. 조각을 따로 맞출 필요가 없습니다.</p>
              </>
            ) : (
              <Puzzle vocab={vocab} pieces={pieces} roles={roles} onRole={setRole} onSwap={swapRoles} />
            ))}
        </div>
      )}
      <div className="bottom-space" aria-hidden="true" />
    </section>
  );
}

function PatternForm({
  includes,
  exclude,
  recursive,
  setIncludes,
  setExclude,
  setRecursive,
  children,
}: {
  includes: Record<LogKind, string>;
  exclude: string;
  recursive: boolean;
  setIncludes: (v: Record<LogKind, string>) => void;
  setExclude: (v: string) => void;
  setRecursive: (v: boolean) => void;
  children?: ReactNode;
}) {
  return (
    <div className="form-grid">
      <label htmlFor="inc-access">접근 로그</label>
      <input id="inc-access" value={includes.access} onChange={(e) => setIncludes({ ...includes, access: e.target.value })} spellCheck={false} />
      <label htmlFor="inc-error">에러 로그</label>
      <input id="inc-error" value={includes.error} onChange={(e) => setIncludes({ ...includes, error: e.target.value })} spellCheck={false} />
      <label htmlFor="exclude">제외</label>
      <input id="exclude" value={exclude} onChange={(e) => setExclude(e.target.value)} spellCheck={false} />
      <label />
      <div className="row">
        <label className="check">
          <input type="checkbox" checked={recursive} onChange={(e) => setRecursive(e.target.checked)} /> 하위 폴더 포함
        </label>
        {children}
      </div>
    </div>
  );
}

/** 머리줄 요약: 모두 확인됐으면 한마디, 아니면 문제 수. */
function fileSummary(files: ScannedFile[], majority: string): ReactNode {
  const off = files.filter((f) => f.best_profile !== majority).length;
  if (off === 0) return <span className="ok"> · 모두 같은 포맷</span>;
  return <span className="warn"> · {formatCount(off)}개는 포맷이 다르거나 판별되지 않음</span>;
}

function FileRow({ f, root, majority, checked, onToggle }: { f: ScannedFile; root: string; majority: string | null; checked: boolean; onToggle: () => void }) {
  let flag: { cls: string; text: string; title: string } | null = null;
  if (f.detect_error) flag = { cls: "bad", text: "읽기 실패", title: f.detect_error };
  else if (f.file_size === 0) flag = { cls: "empty", text: "데이터 없음", title: "빈 파일입니다" };
  else if (majority === null) flag = null;
  else if (!f.best_profile) flag = { cls: "warn", text: "판별 안 됨", title: "선두 줄이 어떤 포맷과도 맞지 않습니다" };
  else if (f.best_profile !== majority) flag = { cls: "warn", text: "다른 포맷", title: "다른 파일들과 포맷이 다릅니다. 포맷이 같은 파일끼리 나눠서 가져오세요." };
  const name = relativeTo(root, f.path);
  return (
    <label className={`file-card ${checked ? "sel" : ""} ${flag ? flag.cls : ""}`} role="listitem" title={flag ? `${f.path}\n${flag.title}` : f.path}>
      <input type="checkbox" checked={checked} onChange={onToggle} aria-label={`${name} 선택`} />
      <span className="checkmark" aria-hidden="true" />
      <span className="file-name mono">{name}</span>
      {flag && <span className="file-flag">{flag.text}</span>}
      <span className="file-size num">{formatBytes(f.file_size)}</span>
    </label>
  );
}

function dragPayload(e: DragEvent): { kind: "piece"; index: number } | { kind: "role"; role: Role } | null {
  const t = e.dataTransfer.getData("text/plain");
  if (t.startsWith("piece:")) return { kind: "piece", index: Number(t.slice(6)) };
  if (t.startsWith("role:")) return { kind: "role", role: t.slice(5) as Role };
  return null;
}

/** 헤딩 옆 한 줄 상태. 샘플 기준이며 전체 파일의 정확도가 아니다. */
function matchNote(p: PreviewResult): string {
  if (p.errors === 0 && p.records > 0) return `선두 ${p.lines_checked}줄 모두 파싱됨`;
  const first = p.outcomes.find((o) => o.kind === "error");
  const where = first && first.kind === "error" ? ` · 첫 오류 ${first.line_number}번째 줄 ${first.code}${first.field ? ` (${first.field})` : ""}` : "";
  return `선두 ${p.lines_checked}줄 중 오류 ${p.errors}${where}`;
}

/** 아웃라인 알약 라벨. 끌어서 조각에 놓는다. */
function RolePill({ def, dragData, effect }: { def: RoleDef; dragData: string; effect: "copy" | "move" }) {
  return (
    <span
      className={`pill kind-${def.kind}`}
      draggable
      title={def.hint}
      onDragStart={(e) => {
        e.dataTransfer.setData("text/plain", dragData);
        e.dataTransfer.effectAllowed = effect;
      }}
    >
      {def.label}
    </span>
  );
}

const UNKNOWN_DEF: RoleDef = { id: "ignore", label: "무시", hint: "저장하지 않음", group: "misc", kind: "ignore" };

/** 원문 한 줄을 조각별로 하이라이트하고 위에 라벨을 얹는다. 라벨을 끌어 조각에 놓거나 조각끼리 끌어 놓아 바꾼다. */
function Puzzle({
  vocab,
  pieces,
  roles,
  onRole,
  onSwap,
}: {
  vocab: RoleDef[];
  pieces: Piece[];
  roles: RoleAssign[];
  onRole: (i: number, r: Role) => void;
  onSwap: (a: number, b: number) => void;
}) {
  const [over, setOver] = useState<number | null>(null);
  const restAt = restIndex(roles, vocab);
  const restDef = restAt === null ? null : roleDef(vocab, roles[restAt].role);
  const tails = restAt !== null && restDef?.tail ? tailLabels(pieces, restAt, restDef.tail) : new Map<number, { label: string; kind: string }>();
  const drop = (i: number) => (e: DragEvent<HTMLSpanElement>) => {
    e.preventDefault();
    setOver(null);
    const p = dragPayload(e);
    if (!p) return;
    if (p.kind === "piece") onSwap(p.index, i);
    else onRole(i, p.role);
  };
  return (
    <>
      <div className="line" role="list" aria-label="조각">
        {pieces.map((p, i) => {
          const absorbed = restAt !== null && i > restAt;
          const def = absorbed ? (roleDef(vocab, roles[restAt].role) ?? UNKNOWN_DEF) : (roleDef(vocab, roles[i]?.role ?? "ignore") ?? UNKNOWN_DEF);
          const tail = tails.get(i);
          return (
            <span
              key={i}
              role="listitem"
              className={`chunk kind-${tail ? tail.kind : def.kind} ${absorbed ? "absorbed" : ""} ${tail?.label ? "tail-key" : ""} ${over === i ? "over" : ""}`}
              draggable
              title={def.hint}
              onDragStart={(e) => {
                e.dataTransfer.setData("text/plain", `piece:${i}`);
                e.dataTransfer.effectAllowed = "move";
              }}
              onDragOver={(e) => {
                e.preventDefault();
                if (over !== i) setOver(i);
              }}
              onDragLeave={() => over === i && setOver(null)}
              onDrop={drop(i)}
            >
              <span className="chunk-label">{absorbed ? (tail?.label ?? "") : def.label}</span>
              <span className="chunk-text">{p.text === "" ? "∅" : p.text}</span>
            </span>
          );
        })}
      </div>
      <div className="role-palette" aria-label="라벨">
        <div className="role-group">
          {vocab
            .filter((d) => !d.rare)
            .map((d) => (
              <RolePill key={d.id} def={d} dragData={`role:${d.id}`} effect="copy" />
            ))}
        </div>
        {vocab.some((d) => d.rare) && (
          <details className="role-more">
            <summary>그 외 {vocab.filter((d) => d.rare).length}개</summary>
            <div className="role-group">
              {vocab
                .filter((d) => d.rare)
                .map((d) => (
                  <RolePill key={d.id} def={d} dragData={`role:${d.id}`} effect="copy" />
                ))}
            </div>
          </details>
        )}
      </div>
    </>
  );
}
