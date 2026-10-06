export function normalize(raw: string): string {
  return raw.trim().toLowerCase();
}

export function slugify(raw: string): string {
  return normalize(raw).replace(/[^a-z0-9]+/g, "-");
}
