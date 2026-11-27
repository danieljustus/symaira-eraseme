package confirmation

import (
	"context"
	"errors"
	"strings"
	"testing"
)

func TestExtractConfirmationLinksFiltersDeduplicatesAndSorts(t *testing.T) {
	links := ExtractConfirmationLinks("https://evil.example/confirm http://acxiom.com/insecure https://www.acxiom.com/long-confirm https://acxiom.com/confirm. https://acxiom.com/confirm")
	if len(links) != 2 || links[0] != "https://acxiom.com/confirm" || links[1] != "https://www.acxiom.com/long-confirm" {
		t.Fatalf("links = %#v", links)
	}
}

func TestAutoConfirmDryRunSkipsClicker(t *testing.T) {
	called := false
	result, err := AutoConfirm(context.Background(), Options{RequestID: 7, ReplyBody: "Click https://acxiom.com/confirm", DryRun: true, Click: func(context.Context, string, ClickOptions) (Result, error) { called = true; return Result{}, nil }})
	if err != nil || !result.Success || result.Step != "dry_run" || !result.DryRun || called {
		t.Fatalf("result=%#v err=%v called=%v", result, err, called)
	}
}

func TestAutoConfirmReportsClickError(t *testing.T) {
	result, err := AutoConfirm(context.Background(), Options{ReplyBody: "https://acxiom.com/confirm", Click: func(ctx context.Context, url string, _ ClickOptions) (Result, error) {
		if _, ok := ctx.Deadline(); !ok {
			t.Fatal("confirmation clicker context has no deadline")
		}
		return Result{ClickedURL: url}, errors.New("net::ERR_CONNECTION_REFUSED https://acxiom.com/confirm?token=secret")
	}})
	if err == nil || result.ClickedURL != "https://acxiom.com/confirm" || result.Error == "" || strings.Contains(result.Error, "token=secret") {
		t.Fatalf("result=%#v err=%v", result, err)
	}
}

func TestAutoConfirmDoesNotTrustSenderControlledDomain(t *testing.T) {
	called := false
	result, err := AutoConfirm(context.Background(), Options{
		ReplyBody:   "https://custom.example/confirm",
		FromAddress: "broker@custom.example",
		Click: func(context.Context, string, ClickOptions) (Result, error) {
			called = true
			return Result{Success: true}, nil
		},
	})
	if err != nil || result.Step != "no_links" || called {
		t.Fatalf("result=%#v err=%v called=%v", result, err, called)
	}
}
