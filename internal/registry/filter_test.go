package registry

import "testing"

func boolPointer(value bool) *bool { return &value }

func TestFilterBrokersAppliesContractDefaultsAndAllFilters(t *testing.T) {
	brokers := []Broker{
		{ID: "active", Jurisdictions: []string{"DE"}, Laws: []string{"GDPR"}, Priority: "high", Category: "credit", Status: "active"},
		{ID: "default-active", Jurisdictions: []string{"US"}, Laws: []string{"CCPA"}, Priority: "medium", Category: "people-search"},
		{ID: "disabled", Jurisdictions: []string{"DE"}, Laws: []string{"GDPR"}, Priority: "high", Category: "credit", Disabled: boolPointer(true)},
		{ID: "deprecated", Jurisdictions: []string{"DE"}, Laws: []string{"GDPR"}, Priority: "low", Category: "analytics", Status: "deprecated"},
	}

	got := FilterBrokers(brokers, BrokerFilter{})
	if len(got) != 2 || got[0].ID != "active" || got[1].ID != "default-active" {
		t.Fatalf("default filter = %#v, want active non-disabled brokers", got)
	}

	cases := []struct {
		name   string
		filter BrokerFilter
		want   string
	}{
		{"jurisdiction", BrokerFilter{Jurisdiction: "DE"}, "active"},
		{"law", BrokerFilter{Law: "CCPA"}, "default-active"},
		{"priority", BrokerFilter{Priority: "high"}, "active"},
		{"category", BrokerFilter{Category: "people-search"}, "default-active"},
		{"status", BrokerFilter{Status: "deprecated"}, "deprecated"},
		{"include inactive", BrokerFilter{IncludeInactive: true, Status: "deprecated"}, "active,default-active,deprecated"},
		{"include disabled", BrokerFilter{IncludeDisabled: true}, "active,default-active,disabled"},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			got := FilterBrokers(brokers, tc.filter)
			if tc.want == "" {
				if len(got) != 0 {
					t.Fatalf("got %#v, want no default-active match", got)
				}
				return
			}
			wantIDs := tc.want
			ids := ""
			for _, broker := range got {
				if ids != "" {
					ids += ","
				}
				ids += broker.ID
			}
			if ids != wantIDs {
				t.Fatalf("ids = %q, want %q", ids, wantIDs)
			}
		})
	}
}

func TestFilterBrokersPreservesOrderAndRejectsMissingMembership(t *testing.T) {
	brokers := []Broker{
		{ID: "one", Jurisdictions: []string{"DE", "AT"}, Laws: []string{"GDPR"}},
		{ID: "two", Jurisdictions: []string{"US"}, Laws: []string{"CCPA"}},
	}
	got := FilterBrokers(brokers, BrokerFilter{Jurisdiction: "AT", Law: "GDPR"})
	if len(got) != 1 || got[0].ID != "one" {
		t.Fatalf("combined filter = %#v", got)
	}
	if got := FilterBrokers(brokers, BrokerFilter{Jurisdiction: "FR"}); len(got) != 0 {
		t.Fatalf("missing jurisdiction matched %#v", got)
	}
}
