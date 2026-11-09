/** Helpers for editing the `field:value` search text. The search box and filter panel share it. */

export interface QToken {
  raw: string;
  negated: boolean;
  field: string;
  op: string;
  value: string;
}

export function tokenize(q: string): string[] {
  return q.match(/(?:[^\s"]+|"[^"]*")+/g) ?? [];
}

export function parseToken(raw: string): QToken | null {
  const m = /^([-!]?)([a-zA-Z_-]+)(>=|<=|:|>|<|=)(.*)$/.exec(raw);
  if (!m) return null;
  return { raw, negated: m[1] !== "", field: m[2].toLowerCase(), op: m[3], value: m[4].replace(/^"|"$/g, "") };
}

function quoted(v: string): string {
  return /[\s,]/.test(v) ? `"${v}"` : v;
}

export function tokensOf(q: string): QToken[] {
  return tokenize(q).map(parseToken).filter((t): t is QToken => t !== null);
}

export function fieldValue(q: string, field: string, op = ":"): string {
  return tokensOf(q).find((t) => t.field === field && t.op === op && !t.negated)?.value ?? "";
}

/** Replaces any token for this field and operator. An empty value removes it. */
export function setField(q: string, field: string, op: string, value: string): string {
  const kept = tokenize(q).filter((raw) => {
    const t = parseToken(raw);
    return !(t && t.field === field && t.op === op && !t.negated);
  });
  if (value.trim()) kept.push(`${field}${op}${quoted(value.trim())}`);
  return kept.join(" ");
}

export function hasValue(q: string, field: string, value: string, negated: boolean): boolean {
  return tokensOf(q).some((t) => t.field === field && t.op === ":" && t.negated === negated && t.value.split(",").includes(value));
}

/** Adds or removes one value, keeping any other values listed with it. */
export function toggleValue(q: string, field: string, value: string, negated = false): string {
  if (!hasValue(q, field, value, negated)) {
    const next = tokenize(q).filter((raw) => {
      const t = parseToken(raw);
      return !(t && t.field === field && t.op === ":" && t.negated !== negated && t.value.split(",").includes(value));
    });
    next.push(`${negated ? "-" : ""}${field}:${quoted(value)}`);
    return next.join(" ");
  }
  const out: string[] = [];
  for (const raw of tokenize(q)) {
    const t = parseToken(raw);
    if (t && t.field === field && t.op === ":" && t.negated === negated && t.value.split(",").includes(value)) {
      const left = t.value.split(",").filter((v) => v !== value);
      if (left.length) out.push(`${negated ? "-" : ""}${field}:${left.join(",")}`);
    } else out.push(raw);
  }
  return out.join(" ");
}

export function removeToken(q: string, raw: string): string {
  return tokenize(q).filter((t) => t !== raw).join(" ");
}
