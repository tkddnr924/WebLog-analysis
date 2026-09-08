import { describe, expect, it } from "vitest";
import { completions, highlightLines } from "./yaraHighlight";

describe("highlightLines", () => {
  it("classifies keywords, fields, strings, regex, vars, numbers and comments", () => {
    const [l1, l2, l3] = highlightLines('rule r { // c\n  $a = /x\\/y/i nocase\n  status in 200..299 and path contains "q" /* b');
    expect(l1.map((t) => t.kind)).toEqual(["keyword", "text", "text", "text", "punct", "text", "comment"]);
    expect(l2.filter((t) => t.kind !== "text").map((t) => [t.kind, t.text])).toEqual([["var", "$a"], ["punct", "="], ["regex", "/x\\/y/i"], ["keyword", "nocase"]]);
    const kinds3 = l3.filter((t) => t.kind !== "text").map((t) => t.kind);
    expect(kinds3).toEqual(["field", "keyword", "number", "punct", "punct", "number", "keyword", "field", "keyword", "string", "comment"]);
    expect(l1.map((t) => t.text).join("")).toBe("rule r { // c");
  });

  it("keeps block comments open across lines and does not treat division-like slashes as regex", () => {
    const lines = highlightLines("/* a\nb */ status == 1\npath == \"/x/y\"");
    expect(lines[0][0].kind).toBe("comment");
    expect(lines[1][0].kind).toBe("comment");
    expect(lines[2].find((t) => t.kind === "string")?.text).toBe('"/x/y"');
  });

  it("suggests completions", () => {
    expect(completions("$s", ["sig1", "sig2", "x"])).toEqual(["$sig1", "$sig2"]);
    expect(completions("sta", [])).toEqual(["status", "startswith"]);
    expect(completions("status", [])).toEqual([]);
    expect(completions("", [])).toEqual([]);
  });
});
