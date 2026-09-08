import { describe, expect, it } from "vitest";
import { isEncoded, safeDecode } from "./urldecode";

describe("safeDecode", () => {
  it("decodes percent sequences including multibyte UTF-8", () => {
    expect(safeDecode("/s?q=%3Cscript%3E")).toBe("/s?q=<script>");
    expect(safeDecode("/%ED%95%9C%EA%B8%80")).toBe("/한글");
    expect(safeDecode("/plain/path")).toBe("/plain/path");
  });

  it("keeps malformed sequences and still decodes the valid ones", () => {
    expect(safeDecode("/a%ZZb%20c")).toBe("/a%ZZb c");
    expect(safeDecode("/x%E0%A4%")).toBe("/x%E0%A4%");
  });

  it("reports whether decoding changes anything", () => {
    expect(isEncoded("/a%20b")).toBe(true);
    expect(isEncoded("/a b")).toBe(false);
  });
});
