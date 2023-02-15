package config

import (
	"crypto/sha256"
	"sort"
)

// hash computes a checksum of the input config.
// Note that for the sake of simplicity, this function does not add strong boundaries between keys so it is possible to
// craft two adversarial inputs that will be considered to have the same hash. This is an accepted tradeoff for
// simplicity.
func hash(configs map[string][]byte) []byte {
	keys := make([]string, 0, len(configs))
	for key := range configs {
		keys = append(keys, key)
	}

	sort.Strings(keys)

	sum := sha256.New()
	for _, key := range keys {
		sum.Write([]byte(key))
		sum.Write(configs[key])
		sum.Write([]byte("---"))
	}

	return sum.Sum(nil)
}
