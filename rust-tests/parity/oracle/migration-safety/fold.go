//go:build ignore

// Emits the pinned Go simple-fold mapping used by Windows filepath.sameWord.
// Run only with GOTOOLCHAIN=go1.26.6; no filesystem state is inspected.
package main

import (
	"encoding/json"
	"os"
	"runtime"
	"unicode"
)

func main() {
	pairs := make([][2]int32, 0)
	for r := rune(0); r <= unicode.MaxRune; r++ {
		minimum := r
		for next := unicode.SimpleFold(r); next != r; next = unicode.SimpleFold(next) {
			if next < minimum {
				minimum = next
			}
		}
		if minimum != r {
			pairs = append(pairs, [2]int32{r, minimum})
		}
	}
	out := struct {
		Toolchain string     `json:"toolchain"`
		Unicode   string     `json:"unicode"`
		Pairs     [][2]int32 `json:"pairs"`
	}{runtime.Version(), unicode.Version, pairs}
	if err := json.NewEncoder(os.Stdout).Encode(out); err != nil {
		panic(err)
	}
}
