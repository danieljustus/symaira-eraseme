// Package campaign is the Go port of the Python campaign domain:
// src/symeraseme/core/{planning,execution,batch}.py plus the
// campaign-level orchestration from src/symeraseme/services/campaign.py.
//
// It turns a profile + broker selection into a plan, executes removal
// requests through injected adapters (email/web-form), and batches
// execution the same way the Python implementation does.  The event
// store writes use the shared internal/eventstore facade so a campaign
// planned by Go is indistinguishable from one planned by Python.
package campaign

import (
	"context"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

// Adapters: mirror core/protocols.py (WebFormRunner, EmailSender) so the
// port keeps the same dependency-injection shape and stub adapters can
// drive deterministic event sequences in tests.

// WebFormRunner executes one web-form opt-out for a broker.  dry_run must
// stay side-effect free.
type WebFormRunner func(ctx context.Context, brokerID string, dryRun bool) map[string]any

// EmailSender sends one email message for a removal request and returns
// the adapter result plus any send error (Python's EmailSender raises
// SymerasemeError/OSError; Go returns the error).
type EmailSender func(ctx context.Context, to, subject, body string) (map[string]string, error)

// TemplateRenderer renders a letter template for a request.  The full
// Jinja2→text/template port lives in internal/templating (issue #716);
// this indirection keeps execution portable and testable until then.
type TemplateRenderer func(templateID string, profile *identity.Profile, brokerName string) (string, error)

// PlanOpts mirrors the keyword arguments of core/planning.plan_campaign().
// Empty strings mean "no filter"; Status defaults to "active", matching
// the Python loader default.  MaxBrokers<=0 disables the cap.
type PlanOpts struct {
	CampaignID      string
	Jurisdiction    string
	Law             string
	Priority        string
	Category        string
	Status          string
	IncludeInactive bool
	IncludeDisabled bool
	MaxBrokers      int
	Notes           string
}

// PlanRequest is one planned removal request (the items in the result
// "requests" list of plan_campaign).
type PlanRequest struct {
	RequestID  int64  `json:"request_id"`
	BrokerID   string `json:"broker_id"`
	BrokerName string `json:"broker_name"`
	Channel    string `json:"channel"`
	Template   string `json:"template"`
}

// PlanResult mirrors the dict returned by plan_campaign().
type PlanResult struct {
	CampaignID   string        `json:"campaign_id"`
	TotalBrokers int           `json:"total_brokers"`
	Matched      int           `json:"matched"`
	Planned      int           `json:"planned"`
	Requests     []PlanRequest `json:"requests"`
}

// channel is the selected opt-out channel for one broker, carrying the
// fields planning needs (mirrors the Python _select_channel dicts).
type channel struct {
	typ                  string
	endpoint             string
	template             string
	locale               string
	expectedResponseDays int
	requiredFields       []string
}

// ErrIdentityProfile corresponds to Python's ProfileError raised for
// undecryptable identity profiles.
type ErrIdentityProfile struct{ msg string }

func (e *ErrIdentityProfile) Error() string { return e.msg }

// planCampaign implements the core planning flow against a Store.
func planCampaign(
	ctx context.Context,
	store *eventstore.Store,
	brokers []registry.Broker,
	profileHash string,
	opts PlanOpts,
) (*PlanResult, error) {
	// create_campaign returns False when the id already exists — the
	// Python caller logs a warning and appends to the existing campaign.
	if _, err := store.CreateCampaign(ctx, opts.CampaignID, "initial", opts.Notes); err != nil {
		return nil, err
	}

	status := opts.Status
	if status == "" {
		status = "active"
	}
	filtered := registry.FilterBrokers(brokers, registry.BrokerFilter{
		Jurisdiction:    opts.Jurisdiction,
		Law:             opts.Law,
		Priority:        opts.Priority,
		Category:        opts.Category,
		IncludeDisabled: opts.IncludeDisabled,
		Status:          status,
		IncludeInactive: opts.IncludeInactive,
	})

	type sel struct {
		broker  registry.Broker
		channel channel
	}
	var channels []sel
	for _, b := range filtered {
		ch, ok := selectChannel(b)
		if ok {
			channels = append(channels, sel{broker: b, channel: ch})
		}
	}

	matched := len(channels)
	if opts.MaxBrokers > 0 && len(channels) > opts.MaxBrokers {
		channels = channels[:opts.MaxBrokers]
	}

	res := &PlanResult{
		CampaignID:   opts.CampaignID,
		TotalBrokers: len(filtered),
		Matched:      matched,
	}
	for _, s := range channels {
		templateID := resolveTemplate(s.channel)
		requestID, err := store.CreateRemovalRequest(
			ctx,
			s.broker.ID,
			s.channel.typ,
			opts.CampaignID,
			resolveJurisdiction(s.broker, opts.Jurisdiction),
			templateID,
			profileHash,
		)
		if err != nil {
			return nil, err
		}
		payload := map[string]any{
			"broker_name":            s.broker.Name,
			"broker_website":         s.broker.Website,
			"channel":                s.channel.typ,
			"endpoint":               s.channel.endpoint,
			"template":               templateID,
			"locale":                 s.channel.locale,
			"expected_response_days": s.channel.expectedResponseDays,
		}
		if _, _, err := store.AppendAndProject(
			ctx, requestID, eventstore.EvtPlanned, payload, eventstore.SrcSystem, time.Now().UTC(),
		); err != nil {
			return nil, err
		}
		res.Requests = append(res.Requests, PlanRequest{
			RequestID:  requestID,
			BrokerID:   s.broker.ID,
			BrokerName: s.broker.Name,
			Channel:    s.channel.typ,
			Template:   templateID,
		})
	}
	res.Planned = len(res.Requests)
	return res, nil
}

// selectChannel mirrors Python _select_channel: the FIRST opt-out channel
// that is an email or a web form is used.
func selectChannel(b registry.Broker) (channel, bool) {
	for _, c := range b.OptOut {
		switch c.Type {
		case "email":
			days := 30
			if c.ExpectedResponseDays != nil && *c.ExpectedResponseDays > 0 {
				days = *c.ExpectedResponseDays
			}
			return channel{
				typ:                  "email",
				endpoint:             c.Endpoint,
				template:             c.Template,
				locale:               c.Locale,
				expectedResponseDays: days,
				requiredFields:       c.RequiredFields,
			}, true
		case "web_form":
			// Parity: Python's WebFormOptOut branch produces a dict
			// without template/locale (they are ignored for web forms),
			// so _resolve_template yields "".
			return channel{
				typ:                  "web_form",
				endpoint:             c.URL,
				expectedResponseDays: 30,
			}, true
		}
	}
	return channel{}, false
}

// resolveJurisdiction mirrors Python _resolve_jurisdiction: the requested
// jurisdiction when the broker has it, else the broker's first, else
// "UNKNOWN".
func resolveJurisdiction(b registry.Broker, requested string) string {
	if requested != "" {
		for _, j := range b.Jurisdictions {
			if j == requested {
				return requested
			}
		}
	}
	if len(b.Jurisdictions) > 0 {
		return b.Jurisdictions[0]
	}
	return "UNKNOWN"
}

// resolveTemplate mirrors Python _resolve_template: <template>[.<locale>].md.j2
// for a string template; the list item else "".
func resolveTemplate(ch channel) string {
	if ch.template != "" {
		parts := []string{ch.template}
		if ch.locale != "" {
			parts = append(parts, ch.locale)
		}
		parts = append(parts, "md.j2")
		return join(parts, ".")
	}
	return ""
}

func join(parts []string, sep string) string {
	out := ""
	for i, p := range parts {
		if i > 0 {
			out += sep
		}
		out += p
	}
	return out
}
