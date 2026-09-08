// URL 퍼센트 인코딩 해제. 잘못된 시퀀스가 섞여 있어도 가능한 부분만 풀고 나머지는 그대로 둔다.

/** `%XX`를 풀어 준다. 전체 디코딩이 실패하면 시퀀스 단위로 시도한다. 원문에 %가 없으면 그대로. */
export function safeDecode(text: string): string {
  if (!text.includes("%")) return text;
  try {
    return decodeURIComponent(text);
  } catch {
    // UTF-8 다중 바이트 시퀀스는 묶어서 시도하고, 실패하면 그대로 남긴다.
    return text.replace(/(?:%[0-9a-fA-F]{2})+/g, (seq) => {
      try {
        return decodeURIComponent(seq);
      } catch {
        return seq;
      }
    });
  }
}

/** 디코딩해도 달라지지 않으면 false. 토글 버튼을 보일지 정할 때 쓴다. */
export function isEncoded(text: string): boolean {
  return safeDecode(text) !== text;
}
