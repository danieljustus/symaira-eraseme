package registry

import "bytes"

// preflightYAML is a bounded lexical pass. It deliberately does not replace
// yaml.v3's parser: it bounds bytes, conservative nesting, and document
// markers before yaml.v3 parses the input. Exact node counts, depth, anchors,
// aliases, and nulls are checked on yaml.Node after parsing.
func preflightYAML(source []byte) (int, error) {
	if len(source) > maxDocumentBytes {
		return 0, verr("document exceeds %d bytes", maxDocumentBytes)
	}
	var (
		documents    int
		lexicalNodes int
		flowDepth    int
		blockDepth   []int
		hasContent   bool
		afterDocEnd  bool
		blockScalar  bool
		blockIndent  int
	)
	for _, line := range bytes.Split(source, []byte{'\n'}) {
		trimmed := bytes.TrimSpace(line)
		if len(trimmed) == 0 || trimmed[0] == '#' {
			continue
		}
		indent := leadingSpaces(line)
		markerLine := bytes.TrimSuffix(line, []byte{'\r'})
		if blockScalar {
			if indent > blockIndent {
				continue
			}
			blockScalar = false
		}
		if yamlMarker(markerLine, "---") {
			if documents > 0 || hasContent {
				return 0, verr("yaml: multiple documents are not allowed")
			}
			documents = 1
			hasContent = false
			afterDocEnd = false
			continue
		}
		if yamlMarker(markerLine, "...") {
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
		lineNodes, compactDepth, err := scanYAMLLine(line, &flowDepth, len(blockDepth))
		if err != nil {
			return 0, err
		}
		lexicalNodes += lineNodes
		if lexicalNodes > maxYAMLNodes {
			return 0, verr("yaml: lexical node budget exceeded")
		}
		// A compact block sequence such as "- - - value" represents nested
		// collections on one physical line, so indentation alone cannot bound
		// its depth. Count the additional sequence markers explicitly.
		if len(blockDepth)+flowDepth+compactDepth > maxYAMLDepth {
			return 0, verr("yaml: lexical depth budget exceeded")
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
	return lexicalNodes, nil
}

// scanYAMLLine counts conservative node-like tokens and updates flow nesting.
// It is intentionally lexical and may reject unusual-but-valid syntax early;
// yaml.v3 remains authoritative for actual YAML validity and schema decoding.
func scanYAMLLine(line []byte, flowDepth *int, blockLevels int) (int, int, error) {
	lineNodes := 1
	compactMarkers := compactBlockSequenceMarkers(line)
	compactDepth := 0
	if compactMarkers > 1 {
		compactDepth = compactMarkers - 1
		lineNodes += compactDepth
	}
	var quote byte
	for index := 0; index < len(line); {
		character := line[index]
		if quote != 0 {
			if quote == '\'' {
				if character == '\'' {
					if index+1 < len(line) && line[index+1] == '\'' {
						index += 2
						continue
					}
					quote = 0
				}
				index++
				continue
			}
			if character == '\\' {
				index += 2
				continue
			}
			if character == '"' {
				quote = 0
			}
			index++
			continue
		}
		switch character {
		case '\'', '"':
			if isYAMLNodeStart(line, index) {
				quote = character
			}
		case '#':
			if index == 0 || isYAMLCommentBoundary(line[index-1]) {
				return lineNodes, compactDepth, nil
			}
		case '[', '{':
			if *flowDepth > 0 || isYAMLNodeStart(line, index) {
				*flowDepth++
				lineNodes++
				if blockLevels+compactDepth+*flowDepth > maxYAMLDepth {
					return 0, 0, verr("yaml: lexical depth budget exceeded")
				}
			}
		case ']', '}':
			if *flowDepth > 0 {
				*flowDepth--
			}
		case ',':
			if *flowDepth > 0 {
				lineNodes++
			}
		}
		index++
	}
	return lineNodes, compactDepth, nil
}

// isYAMLNodeStart reports whether index follows syntax that can introduce a
// new YAML node. Punctuation in an already-started plain scalar is data, not
// flow syntax (for example, "notes: hello [world").
func isYAMLNodeStart(line []byte, index int) bool {
	previous := index - 1
	for previous >= 0 && (line[previous] == ' ' || line[previous] == '	') {
		previous--
	}
	if previous < 0 {
		return true
	}
	switch line[previous] {
	case ':', ',', '[', '{':
		return true
	case '-':
		for before := 0; before < previous; before++ {
			if line[before] != ' ' && line[before] != '	' && line[before] != '-' {
				return false
			}
		}
		return true
	default:
		return false
	}
}

func compactBlockSequenceMarkers(line []byte) int {
	index := leadingSpaces(line)
	markers := 0
	for index < len(line) && line[index] == '-' {
		if index+1 < len(line) && line[index+1] != ' ' && line[index+1] != '\t' && line[index+1] != '#' {
			break
		}
		markers++
		index++
		for index < len(line) && (line[index] == ' ' || line[index] == '\t') {
			index++
		}
	}
	return markers
}

func leadingSpaces(line []byte) int {
	count := 0
	for count < len(line) && line[count] == ' ' {
		count++
	}
	return count
}

func isYAMLCommentBoundary(character byte) bool {
	return character == ' ' || character == '\t' || character == '[' || character == '{' || character == ','
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
