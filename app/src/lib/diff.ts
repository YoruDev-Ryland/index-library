// What changes between two file names, as runs of kept / removed / added text, so a preview
// can mark only the parts that change. Word-level: words, runs of spaces and punctuation are
// each one token, compared with a longest-common-subsequence.

export type Run = { text: string; kind: "same" | "del" | "add" };

const tokens = (s: string) => s.match(/\s+|[()[\]{}\-_.,!:;~+&']|[^\s()[\]{}\-_.,!:;~+&']+/g) ?? [];

export function diffNames(a: string, b: string): { from: Run[]; to: Run[] } {
  const x = tokens(a), y = tokens(b);
  const n = x.length, m = y.length;
  // lcs[i][j] = length of the LCS of x[i..] and y[j..]
  const lcs = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  for (let i = n - 1; i >= 0; i--) for (let j = m - 1; j >= 0; j--) lcs[i][j] = x[i] === y[j] ? lcs[i + 1][j + 1] + 1 : Math.max(lcs[i + 1][j], lcs[i][j + 1]);
  const from: Run[] = [], to: Run[] = [];
  const push = (list: Run[], text: string, kind: Run["kind"]) => {
    const last = list[list.length - 1];
    if (last && last.kind === kind) last.text += text; else list.push({ text, kind });
  };
  let i = 0, j = 0;
  while (i < n || j < m) {
    if (i < n && j < m && x[i] === y[j]) { push(from, x[i], "same"); push(to, y[j], "same"); i++; j++; }
    else if (j < m && (i === n || lcs[i][j + 1] >= lcs[i + 1][j])) { push(to, y[j], "add"); j++; }
    else { push(from, x[i], "del"); i++; }
  }
  return { from, to };
}
