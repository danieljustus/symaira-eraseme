// AgentClient is the EraseMe-specific host-agent LLM backend.
//
// It delegates LLM calls to a coding-agent CLI (Claude Code, Hermes or
// GitHub Copilot) instead of requiring a separate API key or local Ollama.
// This is the one provider with no llmkit equivalent — it is genuinely
// EraseMe-specific (the Python AgentLLMClient port), so it stays local to
// this package per issue #721's definition of done.
package llm

import (
	"context"
	"fmt"
	"os"
	"os/exec"
	"strings"
	"time"
)

// agentDef describes a host-agent CLI invocation.
type agentDef struct {
	name       string
	cli        string
	checkCmd   []string
	invokeTmpl []string
}

// agentDefs mirrors Python _AGENT_DEFS exactly (CLI, check, invocation).
var agentDefs = map[string]agentDef{
	"claude": {
		name: "claude", cli: "claude",
		checkCmd:   []string{"claude", "--version"},
		invokeTmpl: []string{"claude", "-p", "{prompt}", "--output-format", "text", "--no-input"},
	},
	"hermes": {
		name: "hermes", cli: "hermes",
		checkCmd:   []string{"hermes", "--version"},
		invokeTmpl: []string{"hermes", "-p", "{prompt}"},
	},
	"copilot": {
		name: "copilot", cli: "gh",
		checkCmd:   []string{"gh", "copilot", "--version"},
		invokeTmpl: []string{"gh", "copilot", "suggest", "-p", "{prompt}"},
	},
}

// agentPreference is the auto-detect order (Claude Code > Hermes > Copilot).
var agentPreference = []string{"claude", "hermes", "copilot"}

const agentSubprocessTimeout = 120 * time.Second

// AgentClient delegates classification to a host coding-agent CLI.
type AgentClient struct {
	BaseClient
	requestedBackend string
	resolvedBackend  string
	availabilityDone bool
	available        bool
}

// NewAgentClient builds the agent-delegating client. agentBackend forces a
// backend (claude/hermes/copilot); empty auto-detects.
func NewAgentClient(model, agentBackend string, costTracker []UsageRecord) *AgentClient {
	req := agentBackend
	if req == "" {
		req = envLookup("SYMERASEME_AGENT_BACKEND")
	}
	return &AgentClient{
		BaseClient:       NewBaseClient(modelOrAuto(model), 3, costTracker),
		requestedBackend: req,
	}
}

func modelOrAuto(model string) string {
	if model == "" {
		return "auto"
	}
	return model
}

// IsAvailable reports whether a host-agent CLI is reachable.
func (a *AgentClient) IsAvailable() bool {
	if a.availabilityDone {
		return a.available
	}
	a.availabilityDone = true
	a.resolvedBackend = a.detectBackend(a.requestedBackend)
	a.available = a.resolvedBackend != ""
	return a.available
}

// detectBackend resolves which agent CLI to use. explicit is "claude",
// "hermes" or "copilot"; empty auto-detects in preference order.
func (a *AgentClient) detectBackend(explicit string) string {
	if explicit != "" {
		def, ok := agentDefs[strings.ToLower(explicit)]
		if ok && cliOnPath(def.cli) {
			return strings.ToLower(explicit)
		}
		return ""
	}
	for _, key := range agentPreference {
		if cliOnPath(agentDefs[key].cli) {
			return key
		}
	}
	return ""
}

// Classify runs the shared retry loop and delegates to the host agent.
func (a *AgentClient) Classify(ctx context.Context, systemPrompt, userPrompt string, opts ClassifyOptions) (string, UsageRecord, error) {
	return a.BaseClient.Classify(ctx, systemPrompt, userPrompt, opts, a.callAPI)
}

func (a *AgentClient) callAPI(ctx context.Context, systemPrompt, userPrompt string, opts ClassifyOptions) (string, UsageRecord, error) {
	if a.resolvedBackend == "" {
		if !a.IsAvailable() {
			return "", UsageRecord{}, &Error{msg: "no host agent CLI detected. Install Claude Code, Hermes or GitHub Copilot CLI, or set SYMERASEME_AGENT_BACKEND"}
		}
	}
	def := agentDefs[a.resolvedBackend]
	combined := combinePrompts(systemPrompt, userPrompt)

	var cmd []string
	for _, part := range def.invokeTmpl {
		cmd = append(cmd, strings.ReplaceAll(part, "{prompt}", combined))
	}
	// Forward model choice when the backend supports it.
	if a.Model != "" && a.Model != "auto" && a.resolvedBackend == "claude" {
		cmd = append(cmd, "--model", a.Model)
	}

	execCtx := ctx
	cancel := func() {}
	if _, ok := ctx.Deadline(); !ok {
		execCtx, cancel = context.WithTimeout(ctx, agentSubprocessTimeout)
	}
	defer cancel()

	c := exec.CommandContext(execCtx, cmd[0], cmd[1:]...)
	c.Env = append(os.Environ(), "TERM=dumb")
	out, err := c.Output()
	if err != nil {
		if execCtx.Err() == context.DeadlineExceeded {
			return "", UsageRecord{}, &Error{msg: fmt.Sprintf("host agent timed out after %s", agentSubprocessTimeout)}
		}
		var ee *exec.ExitError
		if ok := asExitError(err, &ee); ok {
			stderr := strings.TrimSpace(string(ee.Stderr))
			if len(stderr) > 500 {
				stderr = stderr[:500]
			}
			return "", UsageRecord{}, &Error{msg: fmt.Sprintf("host agent exited with code %d: %s", ee.ExitCode(), stderr)}
		}
		return "", UsageRecord{}, &Error{msg: fmt.Sprintf("failed to invoke host agent: %v", err), err: err}
	}
	text := strings.TrimSpace(string(out))
	if text == "" {
		return "", UsageRecord{}, &Error{msg: "host agent returned empty response"}
	}

	rec := UsageRecord{Model: "agent:" + def.name}
	return text, rec, nil
}

// combinePrompts merges system and user prompts, mirroring the Python
// _build_combined_prompt separator layout.
func combinePrompts(systemPrompt, userPrompt string) string {
	return strings.Join([]string{systemPrompt, "", "---", "", userPrompt}, "\n")
}

func (a *AgentClient) Close() error { return nil }

// cliOnPath reports whether a binary is on PATH.
var cliOnPath = func(name string) bool {
	_, err := exec.LookPath(name)
	return err == nil
}
