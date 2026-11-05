// Port of src/symeraseme/core/batch.py — batched campaign execution.
package campaign

import (
	"context"
	"fmt"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

// BatchLimit mirrors _BATCH_LIMIT in batch.py (10).
const BatchLimit = 10

// ExecuteCampaign mirrors core/batch.execute_campaign(): fetches the
// PLANNED requests of a campaign (clamped batch), executes each through
// ExecuteRequest with the injected adapters, and returns the aggregated
// result.  Execution errors are captured per request (success=false),
// never aborting the batch.
func ExecuteCampaign(
	ctx context.Context,
	store *eventstore.Store,
	campaignID string,
	opts ExecuteOpts,
	batchSize int,
) (map[string]any, error) {
	if batchSize > BatchLimit {
		batchSize = BatchLimit
	}
	if batchSize <= 0 {
		batchSize = 5
	}

	repo := eventstore.NewRepository(store)
	status := "PLANNED"
	totalPlanned, err := repo.CountRemovalRequests(ctx, &campaignID, &status)
	if err != nil {
		return nil, err
	}
	limit := batchSize
	batch, err := repo.ListRemovalRequests(ctx, eventstore.ListRemovalRequestsOpts{
		CampaignID: &campaignID,
		Status:     &status,
		Limit:      &limit,
	})
	if err != nil {
		return nil, err
	}

	results := make([]map[string]any, 0, len(batch))
	for _, req := range batch {
		id, _ := req["id"].(int64)
		result, err := ExecuteRequest(ctx, store, id, opts)
		if err != nil {
			result = map[string]any{
				"success":    false,
				"error":      safeError(err),
				"request_id": id,
			}
		}
		results = append(results, result)
	}

	return map[string]any{
		"campaign_id":   campaignID,
		"total_planned": totalPlanned,
		"batch_size":    len(batch),
		"results":       results,
	}, nil
}

// countPlaceholder keeps fmt imported for future formatted errors.
var _ = fmt.Sprintf
