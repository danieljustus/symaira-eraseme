// Package confirmation validates confirmation links from broker replies and
// orchestrates an injected click adapter. It deliberately contains no browser
// or provider implementation; the adapter boundary keeps dry-run and tests
// deterministic and prevents untrusted reply text from choosing arbitrary hosts.
package confirmation

import (
	"context"
	"net/url"
	"regexp"
	"sort"
	"strings"
	"time"
)

const defaultClickTimeout = 60 * time.Second

var URLPattern = regexp.MustCompile(`https?://[^\s<>"']+`)

var KnownBrokerDomains = map[string]struct{}{
	"acxiom.com": {}, "oracle.com": {}, "schufa.de": {}, "beenverified.com": {},
	"spokeo.com": {}, "intelius.com": {}, "whitepages.com": {}, "mylife.com": {},
	"peekyou.com": {}, "pipl.com": {}, "radaris.com": {}, "truepeoplesearch.com": {},
	"ussearch.com": {}, "peoplefinders.com": {}, "instantcheckmate.com": {},
	"truthfinder.com": {}, "addresses.com": {}, "anywho.com": {}, "dexknows.com": {},
	"meridiandata.us": {}, "experian.com": {}, "transunion.com": {}, "equifax.com": {},
}

type Result struct {
	Success                bool
	ClickedURL             string
	ClickedHost            string
	ClickedURLSHA256       string
	Step                   string
	Error                  string
	ScreenshotBefore       string
	ScreenshotAfter        string
	ScreenshotBeforeSHA256 string
	ScreenshotAfterSHA256  string
	ScreenshotBeforeBytes  int
	ScreenshotAfterBytes   int
	DryRun                 bool
	TaskID                 int64
	Instructions           string
	Status                 string
	Reason                 string
	ManualActionRequired   bool
}

type ClickOptions struct {
	RequestID     int64
	Headless      bool
	ScreenshotDir string
}

type Clicker func(context.Context, string, ClickOptions) (Result, error)

type Options struct {
	RequestID     int64
	ReplyBody     string
	FromAddress   string
	Headless      bool
	ScreenshotDir string
	DryRun        bool
	Click         Clicker
}

// ExtractConfirmationLinks returns unique allowed URLs ordered by shortest
// path and then shortest URL, matching confirmation_clicker.py.
func ExtractConfirmationLinks(text string, allowed ...map[string]struct{}) []string {
	allow := KnownBrokerDomains
	if len(allowed) > 0 && allowed[0] != nil {
		allow = allowed[0]
	}
	seen := map[string]struct{}{}
	links := make([]string, 0)
	for _, raw := range URLPattern.FindAllString(text, -1) {
		link := strings.TrimRight(raw, ".,;:!?)]>")
		if _, ok := seen[link]; ok {
			continue
		}
		seen[link] = struct{}{}
		parsed, err := url.Parse(link)
		if err != nil || parsed.Host == "" || parsed.Scheme != "https" {
			continue
		}
		host := strings.ToLower(parsed.Hostname())
		host = strings.TrimPrefix(host, "www.")
		if len(allow) > 0 {
			if _, ok := allow[host]; !ok {
				continue
			}
		}
		links = append(links, link)
	}
	sort.SliceStable(links, func(i, j int) bool {
		pi, _ := url.Parse(links[i])
		pj, _ := url.Parse(links[j])
		if len(pi.Path) != len(pj.Path) {
			return len(pi.Path) < len(pj.Path)
		}
		return len(links[i]) < len(links[j])
	})
	return links
}

// AutoConfirm validates the first trusted link and delegates the side effect
// to Click. No clicker is invoked during dry-run.
func AutoConfirm(ctx context.Context, opts Options) (Result, error) {
	links := ExtractConfirmationLinks(opts.ReplyBody, KnownBrokerDomains)
	if len(links) == 0 {
		return Result{Step: "no_links", Error: "No confirmation links found in reply body"}, nil
	}
	result := Result{ClickedURL: links[0]}
	if opts.DryRun {
		result.Success, result.Step, result.DryRun = true, "dry_run", true
		return result, nil
	}
	if opts.Click == nil {
		result.Step = "manual_confirmation_required"
		result.Status = "manual_confirmation_required"
		result.Reason = "dynamic_form"
		result.Error = "confirmation clicker is not configured"
		return result, nil
	}
	clickCtx, cancel := context.WithTimeout(ctx, defaultClickTimeout)
	defer cancel()
	clicked, err := opts.Click(clickCtx, links[0], ClickOptions{RequestID: opts.RequestID, Headless: opts.Headless, ScreenshotDir: opts.ScreenshotDir})
	if clicked.ClickedURL == "" {
		clicked.ClickedURL = links[0]
	}
	if err != nil && clicked.Error == "" {
		clicked.Error = clickerError(err, clicked.ClickedURL)
	}
	return clicked, err
}

func clickerError(err error, currentURL string) string {
	message := URLPattern.ReplaceAllString(err.Error(), "[REDACTED-URL]")
	lower := strings.ToLower(message)
	kind := "Error"
	if strings.Contains(message, "Timeout") || strings.Contains(lower, "timed out") {
		kind = "Timeout"
	}
	if strings.Contains(lower, "net::") {
		kind = "Network error"
	}
	location := "broker host"
	if parsed, parseErr := url.Parse(currentURL); parseErr == nil && parsed.Hostname() != "" {
		location = parsed.Hostname()
	}
	return kind + " at " + location + ": " + truncate(message, 200)
}
func truncate(value string, limit int) string {
	if len(value) <= limit {
		return value
	}
	return value[:limit]
}
