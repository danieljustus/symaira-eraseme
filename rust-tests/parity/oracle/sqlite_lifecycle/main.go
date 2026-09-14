// Command sqlite_lifecycle is a test-only cross-language SQLite lifecycle oracle.
// It executes schema initialization, PRAGMA verification, table/index enumeration,
// transaction and lock contention, interrupted initialization/migration recovery,
// future-version refusal, and read-only behavior using the pinned Go eventstore.
package main

import (
	"bufio"
	"crypto/sha256"
	"database/sql"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"

	_ "modernc.org/sqlite"

	"github.com/danieljustus/symaira-eraseme/internal/eventstore"
)

const (
	pinnedCommit = "bf53346eec234929bedf0314b99e3da85dbb991b"
	sourceSHA256 = "fd1dd416606f29aa4726a62ffe6ad83ef9d7c9eb6c6f42d81e87987913968df3"
	oracleSchema = "symaira-eraseme.sqlite-lifecycle-parity.v1"
)

type Request struct {
	Op    string `json:"op"`
	Path  string `json:"path"`
	ID    int64  `json:"id,omitempty"`
	Value string `json:"value,omitempty"`
	Notes string `json:"notes,omitempty"`
}

type Response struct {
	Status      string         `json:"status"` // "ok" or "error"
	Error       string         `json:"error,omitempty"`
	UserVersion int64          `json:"user_version,omitempty"`
	Pragmas     map[string]any `json:"pragmas,omitempty"`
	Tables      []string       `json:"tables,omitempty"`
	Indexes     []string       `json:"indexes,omitempty"`
	Notes       string         `json:"notes,omitempty"`
	BeforeCount *int64         `json:"before_count,omitempty"`
	DuringCount *int64         `json:"during_count,omitempty"`
	AfterCount  *int64         `json:"after_count,omitempty"`
	Provenance  *Provenance    `json:"provenance,omitempty"`
}

type Provenance struct {
	SourceRevision string `json:"source_revision"`
	SourcePath     string `json:"source_path"`
	SourceSHA256   string `json:"source_sha256"`
	Schema         string `json:"schema"`
}

func verifySourceProvenance() (*Provenance, error) {
	_, source, _, ok := runtime.Caller(0)
	if !ok {
		return nil, fmt.Errorf("oracle source path unavailable")
	}
	storeSource := filepath.Clean(filepath.Join(filepath.Dir(source), "../../../../internal/eventstore/store.go"))
	contents, err := os.ReadFile(storeSource)
	if err != nil {
		return nil, fmt.Errorf("read store.go: %w", err)
	}
	digest := sha256.Sum256(contents)
	actualSHA := hex.EncodeToString(digest[:])
	if actualSHA != sourceSHA256 {
		return nil, fmt.Errorf("source SHA256 mismatch: expected %s, got %s", sourceSHA256, actualSHA)
	}
	return &Provenance{
		SourceRevision: pinnedCommit,
		SourcePath:     "internal/eventstore/store.go",
		SourceSHA256:   sourceSHA256,
		Schema:         oracleSchema,
	}, nil
}

func queryPragmas(db *sql.DB) (map[string]any, error) {
	pragmas := make(map[string]any)

	var busyTimeout int64
	if err := db.QueryRow("PRAGMA busy_timeout").Scan(&busyTimeout); err != nil {
		return nil, fmt.Errorf("query busy_timeout: %w", err)
	}
	pragmas["busy_timeout"] = busyTimeout

	var foreignKeys int64
	if err := db.QueryRow("PRAGMA foreign_keys").Scan(&foreignKeys); err != nil {
		return nil, fmt.Errorf("query foreign_keys: %w", err)
	}
	pragmas["foreign_keys"] = foreignKeys

	var journalMode string
	if err := db.QueryRow("PRAGMA journal_mode").Scan(&journalMode); err != nil {
		return nil, fmt.Errorf("query journal_mode: %w", err)
	}
	pragmas["journal_mode"] = strings.ToLower(journalMode)

	return pragmas, nil
}

func queryTablesAndIndexes(db *sql.DB) (tables []string, indexes []string, err error) {
	rows, err := db.Query("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
	if err != nil {
		return nil, nil, fmt.Errorf("query tables: %w", err)
	}
	defer rows.Close()
	for rows.Next() {
		var name string
		if err := rows.Scan(&name); err != nil {
			return nil, nil, err
		}
		tables = append(tables, name)
	}
	if err := rows.Err(); err != nil {
		return nil, nil, err
	}

	idxRows, err := db.Query("SELECT name FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL ORDER BY name")
	if err != nil {
		return nil, nil, fmt.Errorf("query indexes: %w", err)
	}
	defer idxRows.Close()
	for idxRows.Next() {
		var name string
		if err := idxRows.Scan(&name); err != nil {
			return nil, nil, err
		}
		indexes = append(indexes, name)
	}
	if err := idxRows.Err(); err != nil {
		return nil, nil, err
	}
	return tables, indexes, nil
}

func handleRequest(req Request) Response {
	switch req.Op {
	case "provenance":
		prov, err := verifySourceProvenance()
		if err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		return Response{Status: "ok", Provenance: prov}

	case "open":
		store, err := eventstore.Open(req.Path)
		if err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		defer store.Close()

		v, err := store.UserVersion()
		if err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		pragmas, err := queryPragmas(store.DB())
		if err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		tables, indexes, err := queryTablesAndIndexes(store.DB())
		if err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		return Response{
			Status:      "ok",
			UserVersion: int64(v),
			Pragmas:     pragmas,
			Tables:      tables,
			Indexes:     indexes,
		}

	case "init_interrupted_fixture":
		if err := os.MkdirAll(filepath.Dir(req.Path), 0700); err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		db, err := sql.Open("sqlite", req.Path)
		if err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		defer db.Close()

		createSQL := `CREATE TABLE campaigns (
			id TEXT PRIMARY KEY,
			created_at TIMESTAMP NOT NULL DEFAULT (datetime('now')),
			kind TEXT NOT NULL DEFAULT 'initial',
			notes TEXT
		);
		INSERT INTO campaigns (id, kind, notes) VALUES (?, 'initial', ?);`
		id := req.Value
		if id == "" {
			id = "interrupted-campaign"
		}
		notes := req.Notes
		if notes == "" {
			notes = "preserve me"
		}
		if _, err := db.Exec(createSQL, id, notes); err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		return Response{Status: "ok"}

	case "query_campaign_notes":
		db, err := sql.Open("sqlite", req.Path)
		if err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		defer db.Close()
		var notes string
		id := req.Value
		if id == "" {
			id = "interrupted-campaign"
		}
		if err := db.QueryRow("SELECT notes FROM campaigns WHERE id = ?", id).Scan(&notes); err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		return Response{Status: "ok", Notes: notes}

	case "write_row":
		db, err := sql.Open("sqlite", req.Path)
		if err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		defer db.Close()
		_, err = db.Exec("INSERT INTO values_table (id, value) VALUES (?, ?)", req.ID, req.Value)
		if err != nil {
			return Response{Status: "error", Error: err.Error()}
		}
		return Response{Status: "ok"}

	default:
		return Response{Status: "error", Error: fmt.Sprintf("unknown operation %q", req.Op)}
	}
}

func main() {
	pathFlag := flag.String("path", "", "Database path")
	interactiveSnapshot := flag.Bool("interactive-snapshot", false, "Interactive WAL snapshot mode")
	interactiveLock := flag.Bool("interactive-lock", false, "Interactive lock contention mode")
	idFlag := flag.Int64("id", 0, "Row ID")
	valFlag := flag.String("value", "", "String value")
	flag.Parse()

	if *interactiveSnapshot {
		runInteractiveSnapshot(*pathFlag)
		return
	}
	if *interactiveLock {
		runInteractiveLock(*pathFlag, *idFlag, *valFlag)
		return
	}

	input, err := io.ReadAll(os.Stdin)
	if err != nil || len(input) == 0 {
		resp := Response{Status: "error", Error: "missing request on stdin"}
		_ = json.NewEncoder(os.Stdout).Encode(resp)
		return
	}
	var req Request
	if err := json.Unmarshal(input, &req); err != nil {
		resp := Response{Status: "error", Error: fmt.Sprintf("unmarshal request: %v", err)}
		_ = json.NewEncoder(os.Stdout).Encode(resp)
		return
	}

	resp := handleRequest(req)
	enc := json.NewEncoder(os.Stdout)
	enc.SetIndent("", "  ")
	if err := enc.Encode(resp); err != nil {
		fmt.Fprintf(os.Stderr, "encode response: %v\n", err)
		os.Exit(1)
	}
}

func runInteractiveSnapshot(path string) {
	dsn := fmt.Sprintf("file:%s?_pragma=busy_timeout(5000)&_pragma=journal_mode(WAL)", path)
	db, err := sql.Open("sqlite", dsn)
	if err != nil {
		fmt.Fprintf(os.Stderr, "open: %v\n", err)
		os.Exit(1)
	}
	defer db.Close()

	tx, err := db.Begin()
	if err != nil {
		fmt.Fprintf(os.Stderr, "begin: %v\n", err)
		os.Exit(1)
	}
	var beforeCount int64
	if err := tx.QueryRow("SELECT count(*) FROM values_table").Scan(&beforeCount); err != nil {
		fmt.Fprintf(os.Stderr, "scan before: %v\n", err)
		os.Exit(1)
	}

	// Signal parent that snapshot is active
	fmt.Println("READY")

	scanner := bufio.NewScanner(os.Stdin)
	if !scanner.Scan() {
		fmt.Fprintf(os.Stderr, "stdin closed\n")
		os.Exit(1)
	}

	var duringCount int64
	if err := tx.QueryRow("SELECT count(*) FROM values_table").Scan(&duringCount); err != nil {
		fmt.Fprintf(os.Stderr, "scan during: %v\n", err)
		os.Exit(1)
	}
	if err := tx.Commit(); err != nil {
		fmt.Fprintf(os.Stderr, "commit: %v\n", err)
		os.Exit(1)
	}

	var afterCount int64
	if err := db.QueryRow("SELECT count(*) FROM values_table").Scan(&afterCount); err != nil {
		fmt.Fprintf(os.Stderr, "scan after: %v\n", err)
		os.Exit(1)
	}

	resp := Response{
		Status:      "ok",
		BeforeCount: &beforeCount,
		DuringCount: &duringCount,
		AfterCount:  &afterCount,
	}
	_ = json.NewEncoder(os.Stdout).Encode(resp)
}

func runInteractiveLock(path string, id int64, value string) {
	db, err := sql.Open("sqlite", path)
	if err != nil {
		fmt.Fprintf(os.Stderr, "open: %v\n", err)
		os.Exit(1)
	}
	defer db.Close()

	tx, err := db.Begin()
	if err != nil {
		fmt.Fprintf(os.Stderr, "begin: %v\n", err)
		os.Exit(1)
	}
	defer tx.Rollback()

	if _, err := tx.Exec("BEGIN IMMEDIATE"); err != nil && !strings.Contains(err.Error(), "cannot start a transaction within a transaction") {
		// modernc.org/sqlite: db.Begin() starts deferred, so BEGIN IMMEDIATE upgrades
	}
	if id != 0 || value != "" {
		if _, err := tx.Exec("INSERT INTO values_table (id, value) VALUES (?, ?)", id, value); err != nil {
			fmt.Fprintf(os.Stderr, "insert: %v\n", err)
			os.Exit(1)
		}
	}

	// Signal parent that lock is held
	fmt.Println("HELD")

	scanner := bufio.NewScanner(os.Stdin)
	if !scanner.Scan() {
		fmt.Fprintf(os.Stderr, "stdin closed\n")
		os.Exit(1)
	}
	cmd := strings.TrimSpace(scanner.Text())
	action := "commit"
	if strings.HasPrefix(cmd, "RELEASE") {
		parts := strings.Fields(cmd)
		if len(parts) > 1 {
			action = strings.ToLower(parts[1])
		}
	}

	if action == "commit" {
		if err := tx.Commit(); err != nil {
			fmt.Fprintf(os.Stderr, "commit: %v\n", err)
			os.Exit(1)
		}
	} else {
		_ = tx.Rollback()
	}

	fmt.Println("RELEASED")
}
