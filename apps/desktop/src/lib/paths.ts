// 경로 표시·조합용 순수 함수. Windows(\)와 POSIX(/) 구분자를 모두 다룬다. 테스트 대상.

function separatorOf(path: string): string {
  return path.includes("\\") && !path.includes("/") ? "\\" : "/";
}

/** 루트 경로 아래에 파일 이름을 붙인다. 루트의 구분자 종류를 따른다. */
export function joinPath(root: string, name: string): string {
  if (root === "") return name;
  if (root.endsWith("/") || root.endsWith("\\")) return root + name;
  return root + separatorOf(root) + name;
}

/** 루트 아래 경로를 루트 기준 상대 경로로. 루트 밖이면 원래 경로를 돌려준다. */
export function relativeTo(root: string, path: string): string {
  if (root === "") return path;
  const base = root.endsWith("/") || root.endsWith("\\") ? root : root + separatorOf(root);
  if (path.startsWith(base)) return path.slice(base.length);
  const alt = base.replaceAll("\\", "/");
  if (path.replaceAll("\\", "/").startsWith(alt)) return path.slice(alt.length);
  return path;
}

/** 파일 이름 안의 숫자를 수로 비교한다(access.log.2 < access.log.10). */
export function compareNatural(a: string, b: string): number {
  return a.localeCompare(b, undefined, { numeric: true, sensitivity: "base" });
}

/** 경로의 마지막 이름. 끝의 구분자는 무시한다. */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]+/).filter((p) => p.length > 0);
  return parts[parts.length - 1] ?? "";
}
