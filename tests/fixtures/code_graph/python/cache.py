"""Fixture module used by the code-graph regression baseline."""

from util import normalize

MAX_ENTRIES = 128


class BaseCache:
    """Shared cache behaviour."""

    def label(self):
        return "base"


class Cache(BaseCache):
    """An in-memory cache."""

    def __init__(self):
        self.entries = {}
        self.hits = 0

    def get(self, key):
        return self.entries.get(normalize(key))

    def put(self, key, value):
        self.entries[normalize(key)] = value
        self.hits += 1


def build_cache():
    return Cache()


def warm_cache(cache, keys):
    count = 0
    for key in keys:
        if cache.get(key) is None:
            cache.put(key, 0)
            count += 1
    return count
