package replies

import (
	"context"
	"fmt"
	"net/url"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/confirmation"
	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/llm"
	"github.com/danieljustus/symaira-eraseme/internal/manualtasks"
	"github.com/danieljustus/symaira-eraseme/internal/triage"
)

type Service struct {
	Store *eventstore.Store
	Repo  *Repository
}

func NewService(store *eventstore.Store) *Service {
	return &Service{Store: store, Repo: NewRepository(store)}
}

type ClassifyRequest struct {
	RequestID                        int64
	BrokerName, BrokerWebsite        string
	OriginalSubject, OriginalSnippet string
	Provider, Model                  string
	Profile                          *identity.Profile
	Client                           llm.Client
	Save                             bool
}

func (s *Service) ClassifyReply(ctx context.Context, req ClassifyRequest) (triage.ClassificationResult, error) {
	reply, err := s.Repo.Latest(ctx, req.RequestID, true)
	if err != nil {
		return triage.ClassificationResult{}, err
	}
	if reply == nil {
		return triage.ClassificationResult{}, fmt.Errorf("no unclassified inbox reply found for request #%d", req.RequestID)
	}
	classifier := triage.NewReplyClassifier(req.Client, req.Profile)
	result, err := classifier.Classify(ctx, triage.ClassifyOptions{
		BrokerName: req.BrokerName, BrokerWebsite: req.BrokerWebsite,
		OriginalSubject: req.OriginalSubject, OriginalSnippet: req.OriginalSnippet,
		ReplySubject: reply.Subject, ReplyBody: reply.Snippet, CacheKey: "broker:" + req.BrokerName,
	})
	if err != nil {
		return result, err
	}
	if req.Save {
		if err := s.Repo.Classify(ctx, reply.ID, result.Label, result.Confidence, result.Summary); err != nil {
			return result, err
		}
		_, _, err = s.Store.AppendAndProject(ctx, req.RequestID, eventstore.EventType(result.EventType), map[string]any{
			"classification": result.Label, "confidence": result.Confidence, "summary": result.Summary,
			"extracted_fields": result.ExtractedFields, "reply_id": reply.ID,
		}, eventstore.SrcSystem, nowUTC())
	}
	return result, err
}

type RebuttalRequest struct {
	RequestID                                    int64
	BrokerName, BrokerWebsite                    string
	OriginalRequestTemplate, OriginalRequestDate string
	Profile                                      *identity.Profile
	Client                                       llm.Client
	Save                                         bool
}

func (s *Service) GenerateRebuttal(ctx context.Context, req RebuttalRequest) (triage.RebuttalResult, error) {
	reply, err := s.Repo.Latest(ctx, req.RequestID, false)
	if err != nil {
		return triage.RebuttalResult{}, err
	}
	message := req.OriginalRequestTemplate
	if reply != nil && reply.Snippet != "" {
		message = reply.Snippet
	}
	result, err := triage.GenerateRebuttal(ctx, triage.RebuttalOptions{
		BrokerName: req.BrokerName, BrokerWebsite: req.BrokerWebsite, BrokerMessage: message,
		OriginalRequestTemplate: req.OriginalRequestTemplate, OriginalRequestDate: req.OriginalRequestDate,
		Profile: req.Profile, Client: req.Client,
	})
	if err != nil {
		return result, err
	}
	if req.Save {
		_, _, err = s.Store.AppendAndProject(ctx, req.RequestID, eventstore.EvtRebuttalSent, map[string]any{
			"template_name": result.TemplateName, "rejection_classification": result.RejectionClassification,
			"confidence": result.Confidence, "llm_used": result.LLMUsed,
			"broker_message_snippet": truncate(message, 200),
		}, eventstore.SrcSystem, nowUTC())
	}
	return result, err
}

type AutoConfirmRequest struct {
	RequestID     int64
	Headless      bool
	ScreenshotDir string
	DryRun        bool
	Click         confirmation.Clicker
}

func (s *Service) AutoConfirm(ctx context.Context, req AutoConfirmRequest) (confirmation.Result, error) {
	reply, err := s.Repo.Latest(ctx, req.RequestID, false)
	if err != nil {
		return confirmation.Result{}, err
	}
	if reply == nil {
		return confirmation.Result{Step: "no_reply", Error: fmt.Sprintf("no inbox reply found for request #%d", req.RequestID), DryRun: req.DryRun}, nil
	}
	result, err := confirmation.AutoConfirm(ctx, confirmation.Options{
		RequestID: req.RequestID, ReplyBody: reply.Snippet, FromAddress: reply.From,
		Headless: req.Headless, ScreenshotDir: req.ScreenshotDir, DryRun: req.DryRun, Click: req.Click,
	})
	if err != nil {
		return result, err
	}
	if !req.DryRun && !result.Success && result.Step == "manual_confirmation_required" && result.ClickedURL != "" {
		brokerID, brokerName := brokerForConfirmation(ctx, s.Store, req.RequestID, result.ClickedURL)
		task, taskErr := manualtasks.Create(ctx, s.Store, manualtasks.CreateOpts{
			RequestID: &req.RequestID, BrokerID: brokerID, BrokerName: brokerName,
			FormURL: result.ClickedURL, Reason: "dynamic_form",
			ExtraInstructions: "Open the confirmation URL and complete the confirmation manually; no click was attempted.",
		})
		if taskErr != nil {
			return result, taskErr
		}
		result.TaskID = task.ID
		result.Instructions = task.Instructions
		result.Status = "manual_action_required"
		result.Reason = "dynamic_form"
		result.ManualActionRequired = true
		// HUMAN_ACTION_REQUIRED is canonical; this expected fallback is not a
		// claimed click or an additional failure note.
		result.Error = ""
	}
	if !req.DryRun {
		if result.Success {
			_, _, err = s.Store.AppendAndProject(ctx, req.RequestID, eventstore.EvtConfirmationLinkClicked, map[string]any{
				"url": result.ClickedURL, "step": result.Step,
				"screenshot_before": result.ScreenshotBefore, "screenshot_after": result.ScreenshotAfter,
			}, eventstore.SrcSystem, nowUTC())
		} else if result.Error != "" {
			_, _, err = s.Store.AppendAndProject(ctx, req.RequestID, eventstore.EvtNoteAdded, map[string]any{
				"note": "Auto-confirm failed: " + result.Error, "url": result.ClickedURL,
			}, eventstore.SrcSystem, nowUTC())
		}
	}
	return result, err
}

func brokerForConfirmation(ctx context.Context, store *eventstore.Store, requestID int64, link string) (string, string) {
	if request, err := store.GetRemovalRequest(ctx, requestID); err == nil && request != nil {
		if brokerID, ok := request["broker_id"].(string); ok && brokerID != "" {
			return brokerID, brokerID
		}
	}
	parsed, err := url.Parse(link)
	if err == nil && parsed.Hostname() != "" {
		return parsed.Hostname(), parsed.Hostname()
	}
	return "", ""
}

func nowUTC() (t time.Time) { return time.Now().UTC() }
func truncate(value string, limit int) string {
	if len(value) <= limit {
		return value
	}
	return value[:limit]
}
