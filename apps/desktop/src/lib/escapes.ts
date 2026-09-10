// 로그의 `\xHH` 이스케이프(nginx가 출력 불가 바이트를 적는 방식)를 풀어 평문으로 보여주고, 이진 프로토콜이면 무엇인지 알려준다.

const HEX = /\\x([0-9a-fA-F]{2})/g;

export function hasHexEscapes(text: string): boolean {
  return /\\x[0-9a-fA-F]{2}/.test(text);
}

/** `\xHH`를 바이트로 풀고 나머지는 UTF-8 문자 그대로 둔다. */
export function decodeHexEscapes(text: string): { bytes: number[]; text: string } {
  const bytes: number[] = [];
  const enc = new TextEncoder();
  // Request targets reach the 64KiB line cap; avoid one huge spread (engine arg/stack limit).
  const pushSlice = (slice: string) => {
    for (const b of enc.encode(slice)) bytes.push(b);
  };
  let last = 0;
  for (const m of text.matchAll(HEX)) {
    if (m.index > last) pushSlice(text.slice(last, m.index));
    bytes.push(parseInt(m[1], 16));
    last = m.index + m[0].length;
  }
  if (last < text.length) pushSlice(text.slice(last));
  return { bytes, text: printable(bytes) };
}

/** 출력 가능한 ASCII와 UTF-8 문자는 그대로, 그 외는 `·`. */
export function printable(bytes: number[]): string {
  // fatal: false never throws; bad bytes become U+FFFD.
  const decoded = new TextDecoder("utf-8", { fatal: false }).decode(new Uint8Array(bytes));
  let out = "";
  for (const ch of decoded) {
    const c = ch.codePointAt(0) ?? 0;
    out += c === 0xfffd || c < 0x20 || (c >= 0x7f && c < 0xa0) ? "·" : ch;
  }
  return out;
}

export interface BinaryInfo {
  /** 사람이 읽는 설명. */
  label: string;
  /** TLS ClientHello의 SNI 호스트(있을 때). */
  sni?: string;
}

/** 앞 바이트로 이진 프로토콜을 알아본다. 모르면 null. */
export function describeBinary(b: number[]): BinaryInfo | null {
  if (b.length >= 6 && b[0] === 0x16 && b[1] === 0x03 && b[2] <= 0x04 && b[5] === 0x01) {
    // 레코드 헤더의 버전은 호환성 때문에 3.1로 적히는 일이 많아, 핸드셰이크 본문의 버전(9~10바이트)으로 본다.
    const minor = b.length > 10 && b[9] === 0x03 ? b[10] : -1;
    const ver = minor >= 0x03 ? "TLS 1.2+" : minor === 0x02 ? "TLS 1.1" : minor === 0x01 ? "TLS 1.0" : "TLS";
    const sni = parseSni(b);
    return { label: `${ver} ClientHello — HTTPS 핸드셰이크를 HTTP 포트로 보냄`, sni: sni ?? undefined };
  }
  if (b.length >= 3 && b[0] === 0x80 && b[2] === 0x01) return { label: "SSLv2 ClientHello(구식 스캐너)" };
  if (b.length >= 2 && b[0] === 0x05 && b[1] >= 0x01 && b[1] <= 0x03) return { label: "SOCKS5 핸드셰이크" };
  if (b.length >= 2 && b[0] === 0x04 && b[1] === 0x01) return { label: "SOCKS4 CONNECT" };
  if (b.length >= 4 && b[0] === 0x00 && b[1] === 0x00 && b[2] === 0x00 && b[3] >= 0x08) return { label: "길이 접두 이진 프로토콜(예: RDP/Java 직렬화)" };
  return null;
}

/** TLS ClientHello에서 server_name 확장(0x0000)의 호스트 이름을 꺼낸다. 형식이 어긋나면 null. */
export function parseSni(b: number[]): string | null {
  try {
    let i = 5; // 레코드 헤더
    if (b[i] !== 0x01) return null;
    i += 4; // 핸드셰이크 헤더(type + len3)
    i += 2 + 32; // version + random
    const sidLen = b[i];
    i += 1 + sidLen;
    const csLen = (b[i] << 8) | b[i + 1];
    i += 2 + csLen;
    const compLen = b[i];
    i += 1 + compLen;
    const extLen = (b[i] << 8) | b[i + 1];
    i += 2;
    const end = Math.min(b.length, i + extLen);
    while (i + 4 <= end) {
      const type = (b[i] << 8) | b[i + 1];
      const len = (b[i + 2] << 8) | b[i + 3];
      i += 4;
      if (type === 0) {
        let j = i + 2; // list length
        if (b[j] !== 0) return null; // host_name
        const nameLen = (b[j + 1] << 8) | b[j + 2];
        j += 3;
        if (j + nameLen > b.length) return null;
        return printable(b.slice(j, j + nameLen));
      }
      i += len;
    }
    return null;
  } catch {
    return null;
  }
}
