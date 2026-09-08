import { describe, expect, it } from "vitest";
import { decodeHexEscapes, describeBinary, hasHexEscapes, parseSni, printable } from "./escapes";

/** 최소 구조의 TLS 1.2 ClientHello(SNI example.com)를 만든다. */
function clientHello(host: string): number[] {
  const name = Array.from(new TextEncoder().encode(host));
  const sni = [0x00, 0x00, 0, name.length + 5, 0, name.length + 3, 0x00, 0, name.length, ...name];
  const body = [0x03, 0x03, ...new Array(32).fill(0xaa), 0, 0, 2, 0x13, 0x01, 1, 0, 0, sni.length, ...sni];
  const hs = [0x01, 0, 0, body.length, ...body];
  return [0x16, 0x03, 0x01, 0, hs.length, ...hs];
}
const esc = (b: number[]) => b.map((x) => `\\x${x.toString(16).padStart(2, "0")}`).join("");

describe("escapes", () => {
  it("decodes \\xHH escapes into bytes and printable text", () => {
    const r = decodeHexEscapes("\\x16\\x03\\x01abc\\x00\\xEA한");
    expect(r.bytes.slice(0, 3)).toEqual([0x16, 0x03, 0x01]);
    expect(r.text).toBe("···abc··한");
    expect(hasHexEscapes("/plain")).toBe(false);
    expect(printable([0x47, 0x45, 0x54, 0x20, 0x2f, 0x0a])).toBe("GET /·");
  });

  it("recognises a TLS ClientHello and extracts the SNI host", () => {
    const b = clientHello("example.com");
    expect(parseSni(b)).toBe("example.com");
    const info = describeBinary(decodeHexEscapes(esc(b)).bytes);
    expect(info?.label).toContain("TLS 1.2+ ClientHello");
    expect(info?.sni).toBe("example.com");
    expect(describeBinary([0x05, 0x01, 0x00])?.label).toContain("SOCKS5");
    expect(describeBinary(Array.from(new TextEncoder().encode("/index.html")))).toBeNull();
    expect(parseSni([0x16, 0x03, 0x01])).toBeNull();
  });
});
