package cache

import "strings"

const MaxEntries = 128

type Storable interface {
	Label() string
}

type Entry struct {
	Key  string
	Hits int
}

type Cache struct {
	entries map[string]Entry
}

func (c *Cache) Label() string {
	return "cache"
}

func (c *Cache) Get(key string) (Entry, bool) {
	entry, ok := c.entries[normalize(key)]
	return entry, ok
}

func (c *Cache) Put(entry Entry) {
	c.entries[normalize(entry.Key)] = entry
}

func BuildCache() *Cache {
	return &Cache{entries: make(map[string]Entry)}
}

func WarmCache(c *Cache, keys []string) int {
	count := 0
	for _, key := range keys {
		if _, ok := c.Get(key); !ok {
			c.Put(Entry{Key: key, Hits: 0})
			count++
		}
	}
	return count
}

func normalize(raw string) string {
	return strings.ToLower(strings.TrimSpace(raw))
}
