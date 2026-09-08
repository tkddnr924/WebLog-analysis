import { describe, expect, it } from "vitest";
import { fieldLabel } from "./fieldLabels";

describe("fieldLabel", () => {
  it("maps storage names from every vocabulary and falls back to the name", () => {
    expect(fieldLabel("remote_user")).toBe("로그인 사용자");
    expect(fieldLabel("upstream_response_time")).toBe("업스트림 응답 시간");
    expect(fieldLabel("pid_tid")).toBe("프로세스#스레드");
    expect(fieldLabel("server")).toBe("서버");
    expect(fieldLabel("ident")).toBe("identd");
    expect(fieldLabel("field_3")).toBe("field_3");
  });
});
