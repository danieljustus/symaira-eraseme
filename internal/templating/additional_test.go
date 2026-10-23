package templating

import (
	"strings"
	"testing"
	"time"

	"github.com/danieljustus/symaira-eraseme/internal/identity"
)

func TestJinjaHelpersCoverFalsyNumbersSlicesAndComparisons(t *testing.T) {
	for _, value := range []any{nil, "", []string{}, []any{}, 0, float64(0)} {
		if got := defaultVal(value, "fallback"); got != "fallback" {
			t.Errorf("defaultVal(%#v) = %#v", value, got)
		}
	}
	if got := first([]string{"first"}); got != "first" || first([]string{}) != "" || first("text") != "t" {
		t.Fatal("first helper mismatch")
	}
	if join([]any{"a", 2}, ",") != "a,2" || length(map[string]any{"a": 1}) != 1 || length(42) != 0 {
		t.Fatal("join/length helper mismatch")
	}
	if subF(8, 3) != 5 || divF(8, 2) != 4 || mulF(3, 2) != 6 || cmpLT(1, 2) != true || notFunc(false) != true {
		t.Fatal("numeric comparison helper mismatch")
	}
	if sliceStr("Äpfel", 0, 2) != "Äp" || sliceStr("abc", 3, 1) != "" {
		t.Fatal("Unicode string slicing mismatch")
	}
	list := []any{"a", "b", "c"}
	if got := sliceList(list, 1, 3); len(got) != 2 || got[0] != "b" {
		t.Fatal("any list slicing mismatch")
	}
	maps := []map[string]any{{"id": 1}, {"id": 2}}
	if got := sliceList(maps, 0, 1); len(got) != 1 || got[0].(map[string]any)["id"] != 1 {
		t.Fatal("map list slicing mismatch")
	}
	if !cmpGE(2, 1) || !cmpLE(1, 2) || !cmpGT(2, 1) || !cmpEQ(2, "2") || !contains("abcdef", "b") {
		t.Fatal("comparison/contains helper mismatch")
	}
	if ternary(true, "yes", "no") != "yes" || ternary(false, "yes", "no") != "no" {
		t.Fatal("ternary helper mismatch")
	}
}

func TestTemplateProfileCommentAndFormattingHelpers(t *testing.T) {
	dob := "1980-01-02"
	vars := profileVars(&identity.Profile{FullName: "Jane", DateOfBirth: &dob, Addresses: []identity.Address{{City: "Berlin"}}, EmailAddresses: []string{"jane@example.test"}})
	if vars["full_name"] != "Jane" || vars["date_of_birth"] != dob || len(vars["addresses"].([]map[string]any)) != 1 {
		t.Fatalf("profile vars = %#v", vars)
	}
	if vars := profileVars(nil); len(vars) != 0 {
		t.Fatal("nil profile produced values")
	}
	comments := protectHTMLComments("<!-- keep --> body /* css */")
	if !strings.Contains(comments, "htmlcomment") || !strings.Contains(comments, "csscomment") {
		t.Fatalf("protected comments = %q", comments)
	}
	if protected := protectDelimitedComments("<!-- unterminated", "<!--", "-->", "htmlcomment"); protected != "<!-- unterminated" {
		t.Fatal("unterminated comment changed")
	}
	when := time.Date(2026, 8, 31, 12, 34, 56, 0, time.FixedZone("CEST", 7200))
	if dateFmt(when, "") != "2026-08-31 10:34 UTC" || dateFmt("raw", "") != "raw" {
		t.Fatal("date formatting mismatch")
	}
	if !strings.Contains(strings.Join(ListTemplateNames(), ","), "gdpr-art17.en.md.j2") {
		t.Fatal("template list missing GDPR template")
	}
	if _, err := Render("unknown-template", RenderOpts{}); err == nil {
		t.Fatal("unknown template accepted")
	}
	if _, err := Render("laws/gdpr-art17.en.md.j2", RenderOpts{ExtraVars: map[string]any{"original_request_date": "2026-08-31"}}); err != nil {
		t.Fatal(err)
	}
}

func TestTemplateNumericAndStatusHelpers(t *testing.T) {
	if add(1, int64(2), float64(3), true, "4") != 11 || intF("4.8") != 4 || roundF(2.6) != 3 {
		t.Fatal("numeric helper conversion mismatch")
	}
	rows := mkstatuses(map[string]any{"planned": 2, "sent": 3, "awaiting_ack": 0, "awaiting_response": 1, "confirmed": 0, "rejected": 0, "overdue": 0})
	if len(rows) != 7 || maxcount(rows) != 3 || maxcount(nil) != 1 {
		t.Fatalf("status rows=%#v max=%d", rows, maxcount(rows))
	}
	if replaceStr("a-b", "-", "/") != "a/b" || !contains("subject", "sub") {
		t.Fatal("string helpers mismatch")
	}
}
