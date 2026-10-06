import { normalize } from "./util";

export interface Storable {
  key: string;
  hits: number;
}

export type CacheKey = string;

export abstract class BaseCache {
  protected entries: Map<CacheKey, Storable> = new Map();

  abstract label(): string;
}

export class Cache extends BaseCache implements Storable {
  key = "cache";
  hits = 0;

  label(): string {
    return "cache";
  }

  get(key: CacheKey): Storable | undefined {
    return this.entries.get(normalize(key));
  }

  put(entry: Storable): void {
    this.entries.set(normalize(entry.key), entry);
    this.hits += 1;
  }
}

export function buildCache(): Cache {
  return new Cache();
}

export const warmCache = (cache: Cache, keys: CacheKey[]): number => {
  let count = 0;
  for (const key of keys) {
    if (cache.get(key) === undefined) {
      cache.put({ key, hits: 0 });
      count += 1;
    }
  }
  return count;
};
