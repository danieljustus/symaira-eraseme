package registry

import (
	"bytes"
)

// preflightYAML is a bounded lexical pass. It deliberately does not replace
// yaml.v3's parser; it prevents untrusted input from reaching that parser when
// its byte, nesting, node, anchor, or document-stream budget is already known
// to be exceeded.
func preflightYAML(source []byte) (int, error) {
	if len(source) > maxDocumentBytes {
		return 0, verr("document exceeds %d bytes", maxDocumentBytes)
	}
	var (
		documents   int
		nodes       int
		flowDepth   int
		blockDepth  []int
		hasContent  bool
		afterDocEnd bool
		blockScalar bool
		blockIndent int
	)
	for _, line := range bytes.Split(source, []byte{'\n'}) {
		trimmed := bytes.TrimSpace(line)
		if len(trimmed) == 0 || trimmed[0] == '#' {
			continue
		}
		indent := leadingSpaces(line)
		if blockScalar {
			if indent > blockIndent {
				continue
			}
			blockScalar = false
		}
		if yamlMarker(trimmed, "---") {
			if documents > 0 || hasContent {
				return 0, verr("yaml: multiple documents are not allowed")
			}
			documents = 1
			hasContent = false
			afterDocEnd = false
			continue
		}
		if yamlMarker(trimmed, "...") {
			if documents == 0 {
				documents = 1
			}
			if !hasContent {
				return 0, verr("yaml: empty documents are not allowed")
			}
			afterDocEnd = true
			continue
		}
		if afterDocEnd {
			return 0, verr("yaml: content after document end is not allowed")
		}
		if documents == 0 {
			documents = 1
		}
		hasContent = true

		for len(blockDepth) > 0 && indent < blockDepth[len(blockDepth)-1] {
			blockDepth = blockDepth[:len(blockDepth)-1]
		}
		if len(blockDepth) == 0 || indent > blockDepth[len(blockDepth)-1] {
			blockDepth = append(blockDepth, indent)
		}
		if len(blockDepth)+flowDepth > maxYAMLDepth {
			return 0, verr("yaml: depth budget exceeded")
		}

		lineNodes := 1
		quote := byte(0)
		escaped := false
		comment := false
		for index, character := range line {
			if comment {
				break
			}
			if quote != 0 {
				if quote == '\'' && character == '\'' && index+1 < len(line) && line[index+1] == '\'' {
					continue
				}
				if quote == '"' && escaped {
					escaped = false
					continue
				}
				if quote == '"' && character == '\\' {
					escaped = true
					continue
				}
				if character == quote {
					quote = 0
				}
				continue
			}
			switch character {
			case '\'', '"':
				quote = character
			case '#':
				if index == 0 || isYAMLWhitespace(line[index-1]) {
					comment = true
				}
			case '[', '{':
				flowDepth++
				lineNodes++
				if flowDepth+len(blockDepth) > maxYAMLDepth {
					return 0, verr("yaml: depth budget exceeded")
				}
			case ']', '}':
				flowDepth--
				if flowDepth < 0 {
					return 0, verr("yaml: unbalanced flow nesting")
				}
			case ',':
				if flowDepth > 0 {
					lineNodes++
				}
			case '&', '*':
				if isYAMLReferenceMarker(line, index) {
					return 0, verr("yaml: anchors and aliases are not allowed")
				}
			}
		}
		nodes += lineNodes
		if nodes > maxYAMLNodes {
			return 0, verr("yaml: node budget exceeded")
		}
		if isBlockScalarHeader(trimmed) {
			blockScalar = true
			blockIndent = indent
		}
	}
	if flowDepth != 0 {
		return 0, verr("yaml: unbalanced flow nesting")
	}
	if documents != 1 || !hasContent {
		return 0, verr("yaml: exactly one non-empty document is required")
	}
	return nodes, nil
}

func leadingSpaces(line []byte) int {
	count := 0
	for count < len(line) && line[count] == ' ' {
		count++
	}
	return count
}

func isYAMLWhitespace(character byte) bool {
	return character == ' ' || character == '\t' || character == '[' || character == '{' || character == ','
}

func isYAMLReferenceMarker(line []byte, index int) bool {
	if index+1 >= len(line) {
		return false
	}
	if !(line[index+1] == '_' || line[index+1] == '-' || (line[index+1] >= 'A' && line[index+1] <= 'Z') || (line[index+1] >= 'a' && line[index+1] <= 'z') || (line[index+1] >= '0' && line[index+1] <= '9')) {
		return false
	}
	return index == 0 || isYAMLWhitespace(line[index-1]) || line[index-1] == ':'
}

func yamlMarker(value []byte, marker string) bool {
	if !bytes.HasPrefix(value, []byte(marker)) {
		return false
	}
	return len(value) == len(marker) || value[len(marker)] == ' ' || value[len(marker)] == '\t' || value[len(marker)] == '#'
}

func isBlockScalarHeader(line []byte) bool {
	colon := bytes.IndexByte(line, ':')
	if colon < 0 {
		return false
	}
	value := bytes.TrimSpace(line[colon+1:])
	return len(value) > 0 && (value[0] == '|' || value[0] == '>')
}
