// Package redaction ports Symaira EraseMe's PII detection and redaction
// primitives. Match offsets are byte offsets, deliberately allowing callers
// to replace only the matched bytes while preserving every other byte in a
// file exactly.
package redaction

import (
	"errors"
	"regexp"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

var (
	ErrInputTooLarge   = errors.New("redaction input exceeds the maximum size")
	ErrMatchLimit      = errors.New("redaction match limit exceeded")
	ErrOutputTooLarge  = errors.New("redaction output exceeds the maximum size")
	ErrProfileTooLarge = errors.New("redaction profile exceeds the maximum size")
	ErrInvalidMatch    = errors.New("redaction match is invalid")
)

const (
	maxRedactionInputBytes  = 16 << 20
	maxRedactionMatches     = 100_000
	maxRedactionOutputBytes = 32 << 20
	maxProfileLiteralBytes  = 16 << 10
	maxProfileLiteralCount  = 4_096
	maxProfileTotalBytes    = 1 << 20
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

// CollectMatches is the compatibility wrapper for CollectMatchesChecked. A
// rejected input produces no matches; security-sensitive callers should use
// the checked form so the error is propagated.
func CollectMatches(content string, profiles ...*identity.Profile) []Match {
	matches, _ := CollectMatchesChecked(content, profiles...)
	return matches
}

// CollectMatchesChecked finds bounded, non-overlapping matches.
func CollectMatchesChecked(content string, profiles ...*identity.Profile) ([]Match, error) {
	if len(content) > maxRedactionInputBytes {
		return nil, ErrInputTooLarge
	}
	var profile *identity.Profile
	if len(profiles) > 0 {
		profile = profiles[0]
	}
	if err := validateProfile(profile); err != nil {
		return nil, err
	}
	matches := make([]Match, 0)

	if profile != nil {
		for _, value := range profile.EmailAddresses {
			if err := appendLiteralMatches(&matches, content, value, "Profile Email", "[REDACTED-EMAIL]"); err != nil {
				return nil, err
			}
		}
		for _, value := range profile.PhoneNumbers {
			if err := appendLiteralMatches(&matches, content, value, "Profile Phone", "[REDACTED-PHONE]"); err != nil {
				return nil, err
			}
		}
		if err := appendLiteralMatches(&matches, content, profile.FullName, "Profile Name", "[REDACTED-NAME]"); err != nil {
			return nil, err
		}
		for _, value := range profile.NameVariants {
			if err := appendLiteralMatches(&matches, content, value, "Profile Name", "[REDACTED-NAME]"); err != nil {
				return nil, err
			}
		}
		for _, address := range profile.Addresses {
			for _, item := range []struct {
				value, name, replacement string
			}{{address.Street, "Profile Street", "[REDACTED-STREET]"}, {address.City, "Profile City", "[REDACTED-CITY]"}, {address.PostalCode, "Profile Postal Code", "[REDACTED-POSTAL]"}} {
				if err := appendLiteralMatches(&matches, content, item.value, item.name, item.replacement); err != nil {
					return nil, err
				}
			}
		}
	}

	for _, rule := range defaultRules {
		remaining := maxRedactionMatches - len(matches)
		indexes := rule.Pattern.FindAllStringIndex(content, remaining+1)
		if len(indexes) > remaining {
			return nil, ErrMatchLimit
		}
		for _, index := range indexes {
			value := content[index[0]:index[1]]
			if rule.Name == "SSN" && invalidSSN(value) {
				continue
			}
			if rule.Name == "Email" && !validEmail(value) {
				continue
			}
			matches = append(matches, Match{Rule: rule, Name: rule.Name, Start: index[0], End: index[1], Value: value})
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
	return filtered, nil
}

func validateProfile(profile *identity.Profile) error {
	if profile == nil {
		return nil
	}
	count, total := 0, 0
	check := func(value string) error {
		if value == "" {
			return nil
		}
		if len(value) > maxProfileLiteralBytes || count >= maxProfileLiteralCount || total > maxProfileTotalBytes-len(value) {
			return ErrProfileTooLarge
		}
		count++
		total += len(value)
		return nil
	}
	for _, value := range profile.EmailAddresses {
		if err := check(value); err != nil {
			return err
		}
	}
	for _, value := range profile.PhoneNumbers {
		if err := check(value); err != nil {
			return err
		}
	}
	if err := check(profile.FullName); err != nil {
		return err
	}
	for _, value := range profile.NameVariants {
		if err := check(value); err != nil {
			return err
		}
	}
	for _, address := range profile.Addresses {
		for _, value := range []string{address.Street, address.City, address.PostalCode} {
			if err := check(value); err != nil {
				return err
			}
		}
	}
	return nil
}

func appendLiteralMatches(matches *[]Match, content, value, name, replacement string) error {
	if value == "" {
		return nil
	}
	pattern := regexp.MustCompile(`(?i)` + regexp.QuoteMeta(value))
	remaining := maxRedactionMatches - len(*matches)
	indexes := pattern.FindAllStringIndex(content, remaining+1)
	if len(indexes) > remaining {
		return ErrMatchLimit
	}
	for _, indexes := range indexes {
		*matches = append(*matches, Match{
			Rule: Rule{
				Name: name, Pattern: pattern,
				Replacer: func(string) string { return replacement },
			},
			Name: name, Start: indexes[0], End: indexes[1],
			Value: content[indexes[0]:indexes[1]],
		})
	}
	return nil
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
	if len(parts) != 2 || len(parts[0]) == 0 || len(parts[1]) == 0 {
		return value
	}
	local, domain := parts[0], parts[1]
	visible := local[:1]
	if len(local) > 2 {
		visible += strings.Repeat("*", len(local)-2) + local[len(local)-1:]
	}
	domainParts := strings.Split(domain, ".")
	if len(domainParts) == 0 || len(domainParts[0]) == 0 {
		return value
	}
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
	if len(digits) < 4 {
		return value
	}
	if len(digits) == 11 {
		return "+1-***-***-" + string(digits[len(digits)-4:])
	}
	return "***-***-" + string(digits[len(digits)-4:])
}

func scrubSSN(string) string { return "***-**-****" }
func scrubIBAN(value string) string {
	if len(value) < 6 {
		return value
	}
	return value[:2] + "**" + strings.Repeat("*", len(value)-4) + value[len(value)-4:]
}
func scrubGermanID(value string) string {
	if len(value) < 2 {
		return value
	}
	return "*******" + value[len(value)-2:]
}
func scrubFrenchID(value string) string {
	if len(value) < 3 {
		return value
	}
	return "***" + value[len(value)-3:]
}
func scrubSpanishID(value string) string {
	if len(value) < 1 {
		return value
	}
	return "****-****-" + value[len(value)-1:]
}

func scrubPassport(value string) string {
	match := passportPattern.FindStringSubmatch(value)
	if len(match) < 2 {
		return value
	}
	passport := match[3]
	if len(passport) < 2 {
		return value
	}
	mask := strings.Repeat("*", max(3, len(passport)-2)) + passport[len(passport)-2:]
	return strings.Replace(value, passport, mask, 1)
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}

// Redact applies the compatibility API. Checked callers should use
// RedactChecked and handle its error explicitly; a rejected request returns an
// empty result rather than pretending the input was redacted.
func Redact(content string, profiles ...*identity.Profile) string {
	redacted, _ := RedactChecked(content, profiles...)
	return redacted
}

// RedactChecked redacts content while enforcing input, match, and output
// bounds before allocating the output buffer.
func RedactChecked(content string, profiles ...*identity.Profile) (string, error) {
	redacted, err := RedactBytesChecked([]byte(content), profiles...)
	return string(redacted), err
}

// RedactBytes is the compatibility API. Security-sensitive callers should use
// RedactBytesChecked so limit failures are propagated.
func RedactBytes(content []byte, profiles ...*identity.Profile) []byte {
	redacted, _ := RedactBytesChecked(content, profiles...)
	return redacted
}

func RedactBytesChecked(content []byte, profiles ...*identity.Profile) ([]byte, error) {
	matches, err := CollectMatchesChecked(string(content), profiles...)
	if err != nil {
		return nil, err
	}
	outputLen := len(content)
	for _, match := range matches {
		if match.Start < 0 || match.End < match.Start || match.End > len(content) || match.Start == match.End {
			return nil, ErrInvalidMatch
		}
		replacement := match.Replacement()
		if len(replacement) > maxRedactionOutputBytes {
			return nil, ErrOutputTooLarge
		}
		if match.End-match.Start > outputLen {
			return nil, ErrInvalidMatch
		}
		outputLen -= match.End - match.Start
		if len(replacement) > maxRedactionOutputBytes-outputLen {
			return nil, ErrOutputTooLarge
		}
		outputLen += len(replacement)
	}
	if outputLen > maxRedactionOutputBytes {
		return nil, ErrOutputTooLarge
	}
	out := make([]byte, 0, outputLen)
	position := 0
	for _, match := range matches {
		out = append(out, content[position:match.Start]...)
		out = append(out, match.Replacement()...)
		position = match.End
	}
	out = append(out, content[position:]...)
	return out, nil
}

// RedactText is a descriptive compatibility alias for Redact.
func RedactText(content string, profiles ...*identity.Profile) string {
	return Redact(content, profiles...)
}
