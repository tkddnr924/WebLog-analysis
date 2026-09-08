// 저장 필드 이름 → 사람이 읽는 라벨. 퍼즐 어휘의 저장 이름을 그대로 쓰므로 어휘에서 찾는다.
import { vocabFor, NGINX_ERROR_TAIL, APACHE_ERROR_TAIL } from "./puzzle";
import type { ServerHint } from "../types";

const CORE: Record<string, string> = {
  timestamp: "시간",
  client_ip: "IP",
  request: "요청 라인",
  method: "메서드",
  request_target: "URL",
  protocol: "프로토콜",
  status: "상태코드",
  bytes_sent: "응답 크기",
  referrer: "리퍼러",
  user_agent: "브라우저(UA)",
  message: "메시지",
  level: "레벨",
};

let cache: Map<string, string> | null = null;

function table(): Map<string, string> {
  if (cache) return cache;
  const m = new Map<string, string>(Object.entries(CORE));
  const servers: ServerHint[] = ["nginx", "apache", "unknown"];
  for (const kind of ["access", "error"] as const) {
    for (const server of servers) {
      for (const d of vocabFor(server, kind)) {
        if (d.name && !m.has(d.name)) m.set(d.name, d.label);
      }
    }
  }
  for (const t of [...NGINX_ERROR_TAIL, ...APACHE_ERROR_TAIL]) {
    if (!m.has(t.name)) m.set(t.name, t.label);
  }
  cache = m;
  return m;
}

/** 라벨을 찾지 못하면 이름을 그대로 돌려준다(field_3 같은 위치 이름 포함). */
export function fieldLabel(name: string): string {
  return table().get(name) ?? name;
}
