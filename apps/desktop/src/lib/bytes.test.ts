import { describe, expect, it } from "vitest";
import { formatIn } from "../panels/ByteValue";

describe("formatIn", () => {
  it("shows exact bytes by default and converts on demand", () => {
    expect(formatIn(8701, "B")).toBe("8,701 B");
    expect(formatIn(8701, "KiB")).toBe("8.50 KiB");
    expect(formatIn(150 * 1024 * 1024, "MiB")).toBe("150 MiB");
    expect(formatIn(3 * 1024 ** 3, "GiB")).toBe("3.00 GiB");
    expect(formatIn(1024 ** 4 * 12.345, "TiB")).toBe("12.3 TiB");
  });
});
