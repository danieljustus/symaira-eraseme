package main

import (
	"bytes"
	"strings"
	"testing"

	"github.com/danieljustus/symaira-eraseme/internal/confirmation"
	"github.com/spf13/cobra"
)

func TestWriteWebActionTextReportsManualFallbackHonestly(t *testing.T) {
	cases := []struct {
		name  string
		value any
		want  string
	}{
		{
			name: "web form",
			value: map[string]any{
				"success": false, "status": "manual_action_required", "task_id": int64(7),
				"url": "https://broker.example/optout", "instructions": "Complete the form manually.",
			},
			want: "manual_action_required task_id=7",
		},
		{
			name: "confirmation",
			value: confirmation.Result{
				Step: "manual_confirmation_required", TaskID: 8,
				ClickedURL: "https://broker.example/confirm", Instructions: "Open the link manually.",
			},
			want: "manual_confirmation_required task_id=8",
		},
	}
	for _, testCase := range cases {
		t.Run(testCase.name, func(t *testing.T) {
			var out bytes.Buffer
			cmd := &cobra.Command{}
			cmd.SetOut(&out)
			if err := writeWebActionText(cmd, testCase.value); err == nil {
				t.Fatal("manual fallback returned a successful exit status")
			}
			if !strings.Contains(out.String(), testCase.want) || strings.TrimSpace(out.String()) == "success" {
				t.Fatalf("text output = %q", out.String())
			}
		})
	}
}
