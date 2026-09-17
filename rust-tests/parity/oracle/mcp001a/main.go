// Command mcp001a is a byte oracle for the MCP initialize contract.
package main

import (
	"bytes"
	"context"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"

	"github.com/danieljustus/symaira-eraseme/internal/mcp"
)

const (
	sourceRevision = "a51c7f3c65218924ce1d505ad8389b2216f08c92"
	sourcePath     = "internal/mcp/server.go:180-207,377-389"
)

type fixtureCase struct {
	Name       string  `json:"name"`
	Request    string  `json:"request,omitempty"`
	RequestB64 string  `json:"request_b64,omitempty"`
	Response   *string `json:"response"`
	ParseError bool    `json:"parse_error,omitempty"`
	raw        []byte  `json:"-"`
}

func main() {
	if len(os.Args) == 2 && os.Args[1] == "--fixture" {
		writeFixture()
		return
	}
	raw, err := io.ReadAll(os.Stdin)
	if err != nil {
		fail(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/", bytes.NewReader(raw))
	rec := httptest.NewRecorder()
	mcp.NewServer(nil).ServeHTTP(rec, req)
	result := struct {
		Status  int    `json:"status"`
		BodyB64 string `json:"body_b64"`
	}{Status: rec.Code, BodyB64: base64.StdEncoding.EncodeToString(rec.Body.Bytes())}
	if err := json.NewEncoder(os.Stdout).Encode(result); err != nil {
		fail(err)
	}
}

func writeFixture() {
	cases := []fixtureCase{
		{Name: "initialize_missing_params_numeric_id", Request: `{"jsonrpc":"2.0","id":1,"method":"initialize"}`},
		{Name: "initialize_float_id_normalizes_like_go", Request: `{"jsonrpc":"2.0","id":1.0,"method":"initialize","params":null}`},
		{Name: "initialize_large_number_id_uses_go_decimal_boundary", Request: `{"jsonrpc":"2.0","id":1e20,"method":"initialize"}`},
		{Name: "initialize_small_number_id_uses_go_decimal_boundary", Request: `{"jsonrpc":"2.0","id":1e-7,"method":"initialize"}`},
		{Name: "initialize_negative_zero_id_is_preserved", Request: `{"jsonrpc":"2.0","id":-0,"method":"initialize"}`},
		{Name: "initialize_object_params_string_id", Request: `{"jsonrpc":"2.0","id":"x","method":"initialize","params":{}}`},
		{Name: "initialize_explicit_null_id_is_a_request", Request: `{"jsonrpc":"2.0","id":null,"method":"initialize"}`},
		{Name: "initialize_notification_has_no_response", Request: `{"jsonrpc":"2.0","method":"initialize"}`},
		{Name: "initialize_array_params_rejected", Request: `{"jsonrpc":"2.0","id":"x","method":"initialize","params":[]}`},
		{Name: "initialize_invalid_id_rejected", Request: `{"jsonrpc":"2.0","id":true,"method":"initialize"}`},
		{Name: "initialize_wrong_protocol_rejected", Request: `{"jsonrpc":"1.0","id":1,"method":"initialize"}`},
		{Name: "initialize_empty_method_rejected", Request: `{"jsonrpc":"2.0","id":1,"method":""}`},
		{Name: "unknown_method_is_not_found", Request: `{"jsonrpc":"2.0","id":1,"method":"other"}`},
		{Name: "initialize_int64_max_id_uses_go_float64", Request: `{"jsonrpc":"2.0","id":9223372036854775807,"method":"initialize"}`},
		{Name: "initialize_out_of_range_id_is_invalid_request", Request: `{"jsonrpc":"2.0","id":1e400,"method":"initialize"}`},
		{Name: "initialize_unknown_out_of_range_number_is_ignored", Request: `{"jsonrpc":"2.0","id":1,"method":"initialize","unknown":1e400}`},
		{Name: "initialize_out_of_range_params_is_invalid_params", Request: `{"jsonrpc":"2.0","id":1,"method":"initialize","params":1e400}`},
		{Name: "initialize_out_of_range_jsonrpc_is_invalid_request", Request: `{"jsonrpc":1e400,"id":1,"method":"initialize"}`},
		{Name: "initialize_out_of_range_method_is_invalid_request", Request: `{"jsonrpc":"2.0","id":1,"method":1e400}`},
		{Name: "initialize_duplicate_overflow_id_uses_last_valid_id", Request: `{"jsonrpc":"2.0","id":1e400,"id":1,"method":"initialize"}`},
		{Name: "initialize_lower_fixed_float_boundary", Request: `{"jsonrpc":"2.0","id":1e-6,"method":"initialize"}`},
		{Name: "initialize_lower_exponent_float_boundary", Request: `{"jsonrpc":"2.0","id":1e-7,"method":"initialize"}`},
		{Name: "initialize_upper_exponent_float_boundary", Request: `{"jsonrpc":"2.0","id":1e21,"method":"initialize"}`},
		{Name: "initialize_html_escaped_string_id", Request: "{\"jsonrpc\":\"2.0\",\"id\":\"<>&\\u2028\\u2029\",\"method\":\"initialize\"}"},
		{Name: "initialize_case_insensitive_field_names", Request: `{"JSONRPC":"2.0","ID":1,"METHOD":"initialize","PARAMS":null}`},
		{Name: "initialize_unicode_folded_params_is_invalid_params", Request: "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"paramſ\":1}"},
		{Name: "initialize_unicode_nonfolded_dotless_id_is_notification", Request: "{\"jsonrpc\":\"2.0\",\"ıd\":1,\"method\":\"initialize\"}"},
		{Name: "initialize_duplicate_jsonrpc_type_error_is_sticky", Request: `{"jsonrpc":1,"jsonrpc":"2.0","id":1,"method":"initialize"}`},
		{Name: "initialize_duplicate_jsonrpc_case_insensitive_type_error_is_sticky", Request: `{"JSONRPC":"2.0","jsonrpc":1,"id":1,"method":"initialize"}`},
		{Name: "initialize_duplicate_method_type_error_is_sticky", Request: `{"jsonrpc":"2.0","method":1,"method":"initialize","id":1}`},
		{Name: "initialize_duplicate_method_case_insensitive_type_error_is_sticky", Request: `{"jsonrpc":"2.0","METHOD":"initialize","method":1,"id":1}`},
		{Name: "initialize_duplicate_id_case_insensitive_last_wins", Request: `{"jsonrpc":"2.0","ID":1,"id":"two","method":"initialize"}`},
		{Name: "initialize_duplicate_params_uses_last_object", Request: `{"jsonrpc":"2.0","id":1,"method":"initialize","params":1,"params":{}}`},
		{Name: "initialize_duplicate_params_uses_last_scalar", Request: `{"jsonrpc":"2.0","id":1,"method":"initialize","params":{},"params":1}`},
		{Name: "initialize_duplicate_params_case_insensitive_last_wins", Request: `{"jsonrpc":"2.0","id":1,"method":"initialize","PARAMS":1,"params":{}}`},
		{Name: "initialize_duplicate_jsonrpc_null_preserves_value", Request: `{"jsonrpc":"2.0","JSONRPC":null,"id":1,"method":"initialize"}`},
		{Name: "initialize_duplicate_method_null_preserves_value", Request: `{"jsonrpc":"2.0","method":"initialize","METHOD":null,"id":1}`},
		{Name: "initialize_trailing_comma_is_parse_error", Request: `{"jsonrpc":"2.0","id":1,"method":"initialize",}`},
		{Name: "initialize_vertical_tab_is_parse_error", raw: []byte("{\x0b\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}")},
		{Name: "initialize_nested_params_overflow_is_invalid_params", Request: `{"jsonrpc":"2.0","id":"nested-overflow","method":"initialize","params":{"x":1e400}}`},
		{Name: "initialize_nested_params_array_overflow_is_invalid_params", Request: `{"jsonrpc":"2.0","id":"nested-array-overflow","method":"initialize","params":{"x":[{"y":1e400}]}}`},
		{Name: "initialize_unpaired_jsonrpc_surrogate_is_replaced", Request: `{"jsonrpc":"\ud800","id":1,"method":"initialize"}`},
		{Name: "initialize_unpaired_method_surrogate_is_replaced", Request: `{"jsonrpc":"2.0","id":1,"method":"\ud800"}`},
		{Name: "initialize_unpaired_string_id_surrogate_is_replaced", Request: `{"jsonrpc":"2.0","id":"\ud800","method":"initialize"}`},
		{Name: "initialize_invalid_utf8_string_id_sequence_is_replaced_bytewise", raw: invalidUTF8RequestBytes(`{"jsonrpc":"2.0","id":"x`, `","method":"initialize"}`, []byte{0xe2, 0x82, 0x28})},
		{Name: "initialize_invalid_utf8_params_value_is_replaced", raw: invalidUTF8RequestByte(`{"jsonrpc":"2.0","id":"param-bytes","method":"initialize","params":{"x":"`, `"}}`, 0xff)},
		{Name: "initialize_unpaired_params_value_is_replaced", Request: `{"jsonrpc":"2.0","id":"param-surrogate","method":"initialize","params":{"x":"\ud800"}}`},
		{Name: "initialize_invalid_utf8_jsonrpc_is_replaced", raw: invalidUTF8Request(`{"jsonrpc":"2.`, `0","id":1,"method":"initialize"}`)},
		{Name: "initialize_invalid_utf8_method_is_replaced", raw: invalidUTF8Request(`{"jsonrpc":"2.0","id":1,"method":"init`, `ialize"}`)},
		{Name: "initialize_invalid_utf8_string_id_is_replaced", raw: invalidUTF8Request(`{"jsonrpc":"2.0","id":"`, `","method":"initialize"}`)},
	}
	for index := range cases {
		raw := []byte(cases[index].Request)
		if cases[index].raw != nil {
			raw = cases[index].raw
			cases[index].RequestB64 = base64.StdEncoding.EncodeToString(raw)
		}
		var output bytes.Buffer
		err := mcp.NewServer(nil).ServeStdio(context.Background(), bytes.NewReader(append(raw, '\n')), &output)
		if err != nil {
			cases[index].ParseError = true
		} else if body := output.String(); body != "" {
			cases[index].Response = &body
		}
	}
	output := struct {
		SourceRevision string        `json:"source_revision"`
		SourcePath     string        `json:"source_path"`
		Cases          []fixtureCase `json:"cases"`
	}{SourceRevision: sourceRevision, SourcePath: sourcePath, Cases: cases}
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetIndent("", "  ")
	if err := encoder.Encode(output); err != nil {
		fail(err)
	}
}

func invalidUTF8Request(prefix, suffix string) []byte {
	return invalidUTF8RequestByte(prefix, suffix, 0xc3)
}

func invalidUTF8RequestByte(prefix, suffix string, invalid byte) []byte {
	return invalidUTF8RequestBytes(prefix, suffix, []byte{invalid})
}

func invalidUTF8RequestBytes(prefix, suffix string, invalid []byte) []byte {
	raw := append([]byte(prefix), invalid...)
	return append(raw, []byte(suffix)...)
}

func fail(err error) {
	fmt.Fprintln(os.Stderr, err)
	os.Exit(1)
}
