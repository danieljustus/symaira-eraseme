// Public planning API — port of src/symeraseme/core/planning.py.
package campaign

import (
	"context"
	"errors"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/registry"
)

// PlanCampaign mirrors core/planning.plan_campaign(): scans the registry,
// creates PLANNED events for matching brokers and returns the plan.  The
// identity profile is loaded from ProfilePath ("" = platform default); a
// missing profile yields an empty identity hash (Python FileNotFoundError
// branch), an undecryptable profile surfaces as a typed error.
func PlanCampaign(
	ctx context.Context,
	store *eventstore.Store,
	brokers []registry.Broker,
	opts PlanOpts,
	profilePath string,
) (*PlanResult, error) {
	identityHash, err := identityHashForPlanning(profilePath)
	if err != nil {
		return nil, err
	}
	return planCampaign(ctx, store, brokers, identityHash, opts)
}

// identityHashForPlanning mirrors the planning.py try/except around
// load_profile + hash_profile: FileNotFound -> empty hash, RuntimeError ->
// ProfileError.
func identityHashForPlanning(profilePath string) (string, error) {
	path := profilePath
	if path == "" {
		p, err := identity.DefaultProfilePath()
		if err != nil {
			return "", err
		}
		path = p
	}
	profile, err := identity.LoadProfile(path)
	if err != nil {
		if errors.Is(err, identity.ErrProfileNotFound) {
			return "", nil
		}
		return "", &ErrIdentityProfile{msg: err.Error()}
	}
	return identity.HashProfile(profile), nil
}

// GetPlan mirrors core/planning.get_plan(): list removal requests,
// optionally filtered by campaign/status.
func GetPlan(ctx context.Context, repo *eventstore.Repository, campaignID, status string) (map[string]any, error) {
	var cid, st *string
	if campaignID != "" {
		cid = &campaignID
	}
	if status != "" {
		st = &status
	}
	requests, err := repo.ListRemovalRequests(ctx, eventstore.ListRemovalRequestsOpts{
		CampaignID: cid,
		Status:     st,
	})
	if err != nil {
		return nil, err
	}
	label := campaignID
	if label == "" {
		label = "all"
	}
	return map[string]any{
		"campaign_id": label,
		"total":       len(requests),
		"requests":    requests,
	}, nil
}
