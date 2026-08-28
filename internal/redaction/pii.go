// Package redaction ports Symaira EraseMe's PII detection and redaction
// primitives. Match offsets are byte offsets, deliberately allowing callers
// to replace only the matched bytes while preserving every other byte in a
// file exactly.
package redaction

import (
	"regexp"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

// Rule describes one PII detector and its replacement policy.
type Rule struct {
	Name     string
	Pattern  *regexp.Regexp
	Replacer func(string) string
}

// Match is one non-overlapping PII match in the original content. Start and
// End are byte offsets (End is exclusive), matching regexp.FindAllStringIndex.
type Match struct {
	Rule  Rule
	Name  string
	Start int
	End   int
	Value string
}

// Replacement returns the replacement for the original matched value.
func (m Match) Replacement() string {
	if m.Rule.Replacer == nil {
		return m.Value
	}
	return m.Rule.Replacer(m.Value)
}

var (
	emailPattern    = regexp.MustCompile(`[a-zA-Z0-9.!#$%&'*+/=?^_` + "`" + `{|}~-]{1,64}@[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?` + strings.Repeat(`(\.[a-zA-Z0-9]([a-zA-Z0-9-]{0,61}[a-zA-Z0-9])?){0,10}`, 13))
	phonePattern    = regexp.MustCompile(`(\+?1[\s.-]?)?\(?[0-9]{3}\)?[\s.-]?[0-9]{3}[\s.-]?[0-9]{4}`)
	ssnPattern      = regexp.MustCompile(`\b[0-9]{3}[- ]?[0-9]{2}[- ]?[0-9]{4}\b`)
	ibanPattern     = regexp.MustCompile(`\b[A-Z]{2}[0-9]{2}[A-Z0-9]{11,30}\b`)
	deIDPattern     = regexp.MustCompile(`\b[A-L][0-9]{8}[A-Z]?\b`)
	frIDPattern     = regexp.MustCompile(`\b[12][0-9]{2}(0[1-9]|1[0-2])[0-9]{5}[0-9]{3}([0-9]{2})?\b`)
	esIDPattern     = regexp.MustCompile(`\b[0-9]{8}[A-HJ-NP-TV-Z]\b`)
	passportPattern = regexp.MustCompile(`(?i)(passport|travel\s*document|reisedokument)\s*(#|no|num|number)?\s*[:.]?\s*([A-Z0-9]{6,9})\b`)
)

var defaultRules = []Rule{
	{Name: "IBAN", Pattern: ibanPattern, Replacer: scrubIBAN},
	{Name: "German ID", Pattern: deIDPattern, Replacer: scrubGermanID},
	{Name: "French ID", Pattern: frIDPattern, Replacer: scrubFrenchID},
	{Name: "Spanish ID", Pattern: esIDPattern, Replacer: scrubSpanishID},
	{Name: "Passport", Pattern: passportPattern, Replacer: scrubPassport},
	{Name: "SSN", Pattern: ssnPattern, Replacer: scrubSSN},
	{Name: "Email", Pattern: emailPattern, Replacer: scrubEmail},
	{Name: "Phone", Pattern: phonePattern, Replacer: scrubPhone},
}

// Rules returns the built-in rules in the same order as Python's scrubber.
// The returned slice is independent, but its regexp and replacer values are
// immutable and safe to share.
func Rules() []Rule {
	return append([]Rule(nil), defaultRules...)
}

// DefaultRules is an explicit alias for Rules for callers that prefer the
// configuration-oriented name.
func DefaultRules() []Rule { return Rules() }

// CollectMatches finds profile-aware and general PII matches, then sorts and
// filters them exactly like interactive.py: earliest start first, longest
// match first at a shared start, and earlier rules win ties.
//
// A variadic profile keeps the no-profile call concise while allowing callers
// to pass the already-loaded identity profile without filesystem side effects.
func CollectMatches(content string, profiles ...*identity.Profile) []Match {
	var profile *identity.Profile
	if len(profiles) > 0 {
		profile = profiles[0]
	}
	matches := make([]Match, 0)

	if profile != nil {
		for _, value := range profile.EmailAddresses {
			appendLiteralMatches(&matches, content, value, "Profile Email", "[REDACTED-EMAIL]")
		}
		for _, value := range profile.PhoneNumbers {
			appendLiteralMatches(&matches, content, value, "Profile Phone", "[REDACTED-PHONE]")
		}
		appendLiteralMatches(&matches, content, profile.FullName, "Profile Name", "[REDACTED-NAME]")
		for _, value := range profile.NameVariants {
			appendLiteralMatches(&matches, content, value, "Profile Name", "[REDACTED-NAME]")
		}
		for _, address := range profile.Addresses {
			appendLiteralMatches(&matches, content, address.Street, "Profile Street", "[REDACTED-STREET]")
			appendLiteralMatches(&matches, content, address.City, "Profile City", "[REDACTED-CITY]")
			appendLiteralMatches(&matches, content, address.PostalCode, "Profile Postal Code", "[REDACTED-POSTAL]")
		}
	}

	for _, rule := range defaultRules {
		for _, indexes := range rule.Pattern.FindAllStringIndex(content, -1) {
			value := content[indexes[0]:indexes[1]]
			if rule.Name == "SSN" && invalidSSN(value) {
				continue
			}
			if rule.Name == "Email" && !validEmail(value) {
				continue
			}
			matches = append(matches, Match{
				Rule: rule, Name: rule.Name, Start: indexes[0], End: indexes[1],
				Value: value,
			})
		}
	}

	sort.SliceStable(matches, func(i, j int) bool {
		if matches[i].Start != matches[j].Start {
			return matches[i].Start < matches[j].Start
		}
		return len(matches[i].Value) > len(matches[j].Value)
	})
	filtered := make([]Match, 0, len(matches))
	lastEnd := -1
	for _, match := range matches {
		if match.Start >= lastEnd {
			filtered = append(filtered, match)
			lastEnd = match.End
		}
	}
	return filtered
}

func appendLiteralMatches(matches *[]Match, content, value, name, replacement string) {
	if value == "" {
		return
	}
	pattern := regexp.MustCompile(`(?i)` + regexp.QuoteMeta(value))
	for _, indexes := range pattern.FindAllStringIndex(content, -1) {
		*matches = append(*matches, Match{
			Rule: Rule{
				Name: name, Pattern: pattern,
				Replacer: func(string) string { return replacement },
			},
			Name: name, Start: indexes[0], End: indexes[1],
			Value: content[indexes[0]:indexes[1]],
		})
	}
}

func invalidSSN(value string) bool {
	digits := strings.Map(func(r rune) rune {
		if r >= '0' && r <= '9' {
			return r
		}
		return -1
	}, value)
	return strings.HasPrefix(digits, "000") || strings.HasPrefix(digits, "666") ||
		(strings.HasPrefix(digits, "9") && len(digits) == 9) ||
		digits[3:5] == "00" || digits[5:] == "0000"
}

func validEmail(value string) bool {
	parts := strings.Split(value, "@")
	if len(parts) != 2 {
		return false
	}
	labels := strings.Split(parts[1], ".")
	if len(labels) > 127 {
		return false
	}
	for _, label := range labels {
		if len(label) == 0 || len(label) > 63 || label[0] == '-' || label[len(label)-1] == '-' {
			return false
		}
	}
	return true
}

func scrubEmail(value string) string {
	parts := strings.SplitN(value, "@", 2)
	local, domain := parts[0], parts[1]
	visible := local[:1]
	if len(local) > 2 {
		visible += strings.Repeat("*", len(local)-2) + local[len(local)-1:]
	}
	domainParts := strings.Split(domain, ".")
	domainDisplay := domainParts[0][:1] + ".*"
	if len(domainParts) >= 2 {
		domainDisplay = domainParts[0][:1] + "*." + strings.Join(domainParts[1:], ".")
	}
	return visible + "@" + domainDisplay
}

func scrubPhone(value string) string {
	digits := make([]byte, 0, len(value))
	for i := 0; i < len(value); i++ {
		if value[i] >= '0' && value[i] <= '9' {
			digits = append(digits, value[i])
		}
	}
	if len(digits) == 11 {
		return "+1-***-***-" + string(digits[len(digits)-4:])
	}
	return "***-***-" + string(digits[len(digits)-4:])
}

func scrubSSN(string) string { return "***-**-****" }
func scrubIBAN(value string) string {
	return value[:2] + "**" + strings.Repeat("*", len(value)-4) + value[len(value)-4:]
}
func scrubGermanID(value string) string  { return "*******" + value[len(value)-2:] }
func scrubFrenchID(value string) string  { return "***" + value[len(value)-3:] }
func scrubSpanishID(value string) string { return "****-****-" + value[len(value)-1:] }

func scrubPassport(value string) string {
	match := passportPattern.FindStringSubmatch(value)
	if len(match) < 2 {
		return value
	}
	passport := match[3]
	mask := strings.Repeat("*", max(3, len(passport)-2)) + passport[len(passport)-2:]
	return strings.Replace(value, passport, mask, 1)
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}

// Redact applies every accepted match while preserving all unmatched bytes.
func Redact(content string, profiles ...*identity.Profile) string {
	matches := CollectMatches(content, profiles...)
	if len(matches) == 0 {
		return content
	}
	var out strings.Builder
	out.Grow(len(content))
	position := 0
	for _, match := range matches {
		out.WriteString(content[position:match.Start])
		out.WriteString(match.Replacement())
		position = match.End
	}
	out.WriteString(content[position:])
	return out.String()
}

// RedactText is a descriptive alias for Redact.
func RedactText(content string, profiles ...*identity.Profile) string {
	return Redact(content, profiles...)
}
