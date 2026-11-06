package registry

import (
	"strings"
	"testing"

	"gopkg.in/yaml.v3"
)

func TestPreflightYAMLBoundsCompactBlockNesting(t *testing.T) {
	source := strings.Repeat("- ", maxYAMLDepth+1) + "value\n"
	if _, err := preflightYAML([]byte(source)); err == nil || !strings.Contains(err.Error(), "depth budget") {
		t.Fatalf("expected compact block depth rejection, got %v", err)
	}
}

func TestPreflightYAMLBoundsIndentedMappingsAndFlowNesting(t *testing.T) {
	var mappings strings.Builder
	for depth := 0; depth <= maxYAMLDepth; depth++ {
		mappings.WriteString(strings.Repeat(" ", depth*2))
		mappings.WriteString("key:\n")
	}
	mappings.WriteString(strings.Repeat(" ", (maxYAMLDepth+1)*2) + "value\n")
	if _, err := preflightYAML([]byte(mappings.String())); err == nil || !strings.Contains(err.Error(), "depth budget") {
		t.Fatalf("expected indented mapping depth rejection, got %v", err)
	}

	flow := strings.Repeat("[", maxYAMLDepth+1) + "value" + strings.Repeat("]", maxYAMLDepth+1) + "\n"
	if _, err := preflightYAML([]byte(flow)); err == nil || !strings.Contains(err.Error(), "depth budget") {
		t.Fatalf("expected flow depth rejection, got %v", err)
	}
}

func TestPreflightYAMLHandlesQuotesPlainAndBlockScalars(t *testing.T) {
	source := "quoted: 'O''Reilly'\nplain: value with an apostrophe's mark\npunctuation: hello [world ]world {world }world, [still plain\ncolon: foo:{bar\nurl: https://host/path,[query\nnotes: |\n  block text contains &anchor *alias # comment-looking text\n"
	if _, err := preflightYAML([]byte(source)); err != nil {
		t.Fatalf("valid scalar forms rejected: %v", err)
	}
	var document yaml.Node
	if err := yaml.Unmarshal([]byte(source), &document); err != nil {
		t.Fatalf("test input must be valid YAML: %v", err)
	}
}

func TestPreflightYAMLCountsFlowMappingKeysAndValues(t *testing.T) {
	pairs := strings.Repeat("key: value,", maxYAMLNodes/2)
	source := "mapping: {" + pairs + "last: value}\n"
	if _, err := preflightYAML([]byte(source)); err == nil || !strings.Contains(err.Error(), "node budget") {
		t.Fatalf("expected flow mapping node rejection, got %v", err)
	}
}

func TestPreflightYAMLRejectsTrailingDocuments(t *testing.T) {
	for name, source := range map[string]string{
		"second document alias": "value: first\n---\nvalue: *real\n",
		"second document deep":  "value: first\n---\n" + strings.Repeat("[", maxYAMLDepth+1) + "x" + strings.Repeat("]", maxYAMLDepth+1) + "\n",
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := preflightYAML([]byte(source)); err == nil {
				t.Fatal("adversarial YAML accepted")
			}
		})
	}
}

func TestNodeContractRejectsAnchorsAndAliases(t *testing.T) {
	for name, source := range map[string]string{
		"anchor": "value: &real anchored\n",
		"alias":  "value: *real\n",
	} {
		t.Run(name, func(t *testing.T) {
			if _, err := decodeAndValidate(&doc{id: "test", content: []byte(source)}); err == nil {
				t.Fatal("adversarial YAML accepted")
			}
		})
	}
}

func TestPreflightYAMLRejectsLexicalNodeLimit(t *testing.T) {
	source := "values: [" + strings.Repeat("x,", maxYAMLNodes) + "x]\n"
	if _, err := preflightYAML([]byte(source)); err == nil || !strings.Contains(err.Error(), "node budget") {
		t.Fatalf("expected lexical node rejection, got %v", err)
	}
}

func TestExactYAMLNodeBudgetRejectsOversizedNodeTrees(t *testing.T) {
	sequence := &yaml.Node{Kind: yaml.SequenceNode}
	for range maxYAMLNodes {
		sequence.Content = append(sequence.Content, &yaml.Node{Kind: yaml.ScalarNode, Value: "value"})
	}
	if _, err := exactYAMLNodeBudget(&yaml.Node{Kind: yaml.DocumentNode, Content: []*yaml.Node{sequence}}); err == nil || !strings.Contains(err.Error(), "node budget") {
		t.Fatalf("expected exact yaml.Node limit rejection, got %v", err)
	}
}
