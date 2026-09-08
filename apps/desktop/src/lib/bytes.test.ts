import { describe, expect, it } from "vitest";
import { formatIn } from "../panels/ByteValue";

describe("formatIn", () => {
  it("shows exact bytes by default and converts on demand", () => {
    expect(formatIn(8701, "B")).toBe("8,701 B");
    expect(formatIn(8701, "KB")).toBe("8.50 KB");
    expect(formatIn(150 * 1024 * 1024, "MB")).toBe("150 MB");
    expect(formatIn(3 * 1024 ** 3, "GB")).toBe("3.00 GB");
    expect(formatIn(1024 ** 4 * 12.345, "TB")).toBe("12.3 TB");
  });
});
