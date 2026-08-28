// Package registry (filter.go) ports the Python load_all_brokers()
// filtering semantics (src/symeraseme/registry/loader.py) onto the Go
// broker model.  The loader loads everything; this function applies the
// same precedence and defaults the Python cache path applies.
package registry

// BrokerFilter mirrors the keyword arguments of Python's
// load_all_brokers().  Zero values mean "no filter applied" except
// Status, which defaults to "active" unless IncludeInactive is set —
// exactly like the Python loader (active default, include_inactive
// opt-out).
type BrokerFilter struct {
	Jurisdiction    string
	Law             string
	Priority        string
	Category        string
	IncludeDisabled bool
	Status          string
	IncludeInactive bool
}

// FilterBrokers applies the load_all_brokers filter semantics to a
// loaded broker slice.  Order is preserved.
func FilterBrokers(brokers []Broker, f BrokerFilter) []Broker {
	out := make([]Broker, 0, len(brokers))
	for _, b := range brokers {
		disabled := b.Disabled != nil && *b.Disabled
		if !f.IncludeDisabled && disabled {
			continue
		}
		if f.Jurisdiction != "" && !containsStr(b.Jurisdictions, f.Jurisdiction) {
			continue
		}
		if f.Law != "" && !containsStr(b.Laws, f.Law) {
			continue
		}
		if f.Priority != "" && b.Priority != f.Priority {
			continue
		}
		if f.Category != "" && b.Category != f.Category {
			continue
		}
		status := b.Status
		if status == "" {
			status = "active"
		}
		if !f.IncludeInactive {
			expected := f.Status
			if expected == "" {
				expected = "active"
			}
			if status != expected {
				continue
			}
		}
		out = append(out, b)
	}
	return out
}

func containsStr(xs []string, v string) bool {
	for _, x := range xs {
		if x == v {
			return true
		}
	}
	return false
}
