// Package migration contains the bounded Python-to-Go installation migration.
//
// Migration is deliberately file-oriented and conservative: it detects only
// known Python-era artifacts, makes a complete source backup before the first
// destination write, never removes the source, and records completion per
// artifact so an interrupted run can be resumed. Secret values are not read
// or copied by the default implementation.
package migration

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"

	"github.com/danieljustus/symaira-eraseme/internal/scheduler"
)

const stateVersion = 1

// ArtifactKind identifies one migration surface.
type ArtifactKind string

const (
	KindEventDB   ArtifactKind = "event-db"
	KindConfig    ArtifactKind = "config"
	KindProfile   ArtifactKind = "profile"
	KindSecrets   ArtifactKind = "secret-store"
	KindScheduler ArtifactKind = "scheduler"
)

// Artifact is a planned migration item. Source and Destination are paths for
// file artifacts and opaque labels for the secret-store item. Secret values
// are never represented by this type.
type Artifact struct {
	ID          string       `json:"id"`
	Kind        ArtifactKind `json:"kind"`
	Source      string       `json:"source,omitempty"`
	Destination string       `json:"destination"`
	Reason      string       `json:"reason"`
}

// SecretReport is metadata about a Python-era secret store. Implementations
// must return references/counts only; they must not include secret values.
type SecretReport struct {
	Detected       bool   `json:"detected"`
	ReferenceCount int    `json:"reference_count,omitempty"`
	Description    string `json:"description,omitempty"`
	Migratable     bool   `json:"migratable"`
}

// SecretStore is an explicit seam for operators that have a supported secret
// transfer mechanism. Inspect and Migrate are the only operations involved;
// the migration package never obtains or logs secret material itself.
type SecretStore interface {
	Inspect(context.Context, string) (SecretReport, error)
	Migrate(context.Context, string) error
}

// MetadataOnlySecretStore detects the Python keyring-backed store from local
// metadata only. It intentionally cannot copy secrets. This is the safe
// default used by the CLI and by tests.
type MetadataOnlySecretStore struct{}

func (MetadataOnlySecretStore) Inspect(_ context.Context, sourceRoot string) (SecretReport, error) {
	profile := filepath.Join(sourceRoot, "identity.enc")
	refs := filepath.Join(sourceRoot, "secret-refs.json")
	if !exists(profile) && !exists(refs) {
		return SecretReport{}, nil
	}
	return SecretReport{
		Detected:    true,
		Description: "Python secrets are keyring-backed; values were not inspected",
		Migratable:  false,
	}, nil
}

func (MetadataOnlySecretStore) Migrate(context.Context, string) error {
	return errors.New("secret migration requires an injected SecretStore; secret values are never copied by default")
}

// Options controls detection and migration. SourceRoot and DestinationRoot
// must be separate directories. Scheduler native units can deliberately have
// the same source and destination home because they are backed up before
// replacement.
type Options struct {
	SourceRoot            string
	DestinationRoot       string
	SourceConfigRoot      string
	DestinationConfigRoot string
	BackupDir             string
	HomeDir               string
	Platform              scheduler.Platform
	SchedulerSource       string
	SchedulerDest         string
	BinaryPath            string
	ProjectDir            string
	SchedulerConfig       scheduler.Config
	CopySecrets           bool
	DryRun                bool
	SecretStore           SecretStore
	BeforeItem            func(Artifact) error
}

// Detection describes a Python-era installation without mutating it.
type Detection struct {
	Detected    bool         `json:"detected"`
	Summary     string       `json:"summary"`
	Reasons     []string     `json:"reasons"`
	SecretStore SecretReport `json:"secret_store"`
	Artifacts   []Artifact   `json:"artifacts"`
}

// ItemResult records one resumable migration item.
type ItemResult struct {
	Artifact Artifact `json:"artifact"`
	Status   string   `json:"status"` // planned, done, skipped, manual, failed
	Error    string   `json:"error,omitempty"`
}

// Report is the stable command/API result.
type Report struct {
	Detection Detection    `json:"detection"`
	DryRun    bool         `json:"dry_run"`
	Resumed   bool         `json:"resumed"`
	BackupDir string       `json:"backup_dir,omitempty"`
	Items     []ItemResult `json:"items"`
	Warnings  []string     `json:"warnings,omitempty"`
	Complete  bool         `json:"complete"`
}

type state struct {
	Version         int               `json:"version"`
	SourceRoot      string            `json:"source_root"`
	DestinationRoot string            `json:"destination_root"`
	BackupDir       string            `json:"backup_dir"`
	Items           map[string]string `json:"items"`
}

// Detect inspects synthetic or operator-selected paths only. It does not
// access a keychain and never reads secret values.
func Detect(ctx context.Context, opts Options) (Detection, error) {
	source, dest, err := validateRoots(opts.SourceRoot, opts.DestinationRoot)
	if err != nil {
		return Detection{}, err
	}
	if err := validateAuxiliaryPaths(opts); err != nil {
		return Detection{}, err
	}
	configSource, configDestination, err := configRoots(opts, source, dest)
	if err != nil {
		return Detection{}, err
	}
	if err := validateMigrationPaths(opts, source, dest, configSource, configDestination); err != nil {
		return Detection{}, err
	}
	platform := opts.Platform
	if platform == "" {
		platform = scheduler.DetectPlatform()
	}
	secretStore := opts.SecretStore
	if secretStore == nil {
		secretStore = MetadataOnlySecretStore{}
	}
	secretReport, err := secretStore.Inspect(ctx, source)
	if err != nil {
		return Detection{}, fmt.Errorf("inspect secret store: %w", err)
	}
	if !secretReport.Detected && configSource != source {
		secretReport, err = secretStore.Inspect(ctx, configSource)
		if err != nil {
			return Detection{}, fmt.Errorf("inspect config secret store: %w", err)
		}
	}

	d := Detection{SecretStore: secretReport}
	add := func(a Artifact, reason string) {
		a.Reason = reason
		d.Artifacts = append(d.Artifacts, a)
		d.Detected = true
		d.Reasons = append(d.Reasons, reason)
	}
	if p := filepath.Join(source, "symeraseme.db"); exists(p) {
		add(Artifact{ID: "event-db", Kind: KindEventDB, Source: p, Destination: filepath.Join(dest, "symeraseme.db")}, "Python-era event database found")
	}
	for _, name := range []string{"config.toml", ".symeraseme.toml"} {
		p := filepath.Join(configSource, name)
		if exists(p) {
			add(Artifact{ID: "config:" + name, Kind: KindConfig, Source: p, Destination: filepath.Join(configDestination, name)}, "Python-era configuration found")
		}
	}
	if p := filepath.Join(configSource, "identity.enc"); exists(p) {
		add(Artifact{ID: "profile", Kind: KindProfile, Source: p, Destination: filepath.Join(configDestination, "identity.encrypted")}, "Python-era encrypted identity profile found")
	}
	if secretReport.Detected {
		add(Artifact{ID: "secret-store", Kind: KindSecrets, Source: "keyring://symeraseme", Destination: "keyring://symeraseme"}, secretReport.Description)
	}

	generated, err := generatedSchedulerFiles(opts, platform, dest)
	if err != nil {
		return Detection{}, err
	}
	schedulerDetected, err := addSchedulerArtifacts(&d, opts, platform, generated, source, dest)
	if err != nil {
		return Detection{}, err
	}
	if schedulerDetected {
		d.Detected = true
	}
	sort.Slice(d.Artifacts, func(i, j int) bool { return d.Artifacts[i].ID < d.Artifacts[j].ID })
	if d.Detected {
		d.Summary = fmt.Sprintf("Python-era Symaira EraseMe installation detected (%d migration item(s))", len(d.Artifacts))
	} else {
		d.Summary = "No known Python-era Symaira EraseMe artifacts detected"
	}
	return d, nil
}

func addSchedulerArtifacts(d *Detection, opts Options, platform scheduler.Platform, generated map[string]string, source, dest string) (bool, error) {
	srcDir := opts.SchedulerSource
	if srcDir == "" {
		srcDir = filepath.Join(source, "schedules")
	}
	dstDir := opts.SchedulerDest
	if dstDir == "" {
		dstDir = filepath.Join(dest, "schedules")
	}
	legacy := false
	for name := range generated {
		p := filepath.Join(srcDir, name)
		if !exists(p) {
			continue
		}
		isPython, err := scheduler.DetectLegacyPythonUnit(p)
		if err != nil {
			return false, fmt.Errorf("inspect scheduler file %s: %w", p, err)
		}
		if isPython {
			legacy = true
		}
	}
	if legacy {
		for name := range generated {
			srcPath := filepath.Join(srcDir, name)
			addScheduler(d, name, srcPath, filepath.Join(dstDir, name), exists(srcPath), "Python-era generated scheduler artifact found")
		}
	}

	home := opts.HomeDir
	if home == "" {
		return legacy, nil
	}
	nativeRoot, names := nativeSchedulerPaths(home, platform)
	nativeLegacy := false
	for _, name := range names {
		p := filepath.Join(nativeRoot, name)
		if !exists(p) {
			continue
		}
		isPython, err := scheduler.DetectLegacyPythonUnit(p)
		if err != nil {
			return false, fmt.Errorf("inspect native scheduler unit %s: %w", p, err)
		}
		if isPython {
			nativeLegacy = true
		}
	}
	if nativeLegacy {
		for name := range generated {
			if !isNativeSchedulerFile(name, platform) {
				continue
			}
			srcPath := filepath.Join(nativeRoot, name)
			addScheduler(d, "native:"+name, srcPath, srcPath, exists(srcPath), "Python-era native scheduler unit found")
		}
	}
	return legacy || nativeLegacy, nil
}

func addScheduler(d *Detection, name, source, destination string, sourceExists bool, reason string) {
	a := Artifact{ID: "scheduler:" + name, Kind: KindScheduler, Destination: destination, Reason: reason}
	if sourceExists {
		a.Source = source
	}
	d.Artifacts = append(d.Artifacts, a)
	d.Reasons = append(d.Reasons, reason)
}

func nativeSchedulerPaths(home string, platform scheduler.Platform) (string, []string) {
	switch platform {
	case scheduler.PlatformLaunchd:
		return filepath.Join(home, "Library", "LaunchAgents"), []string{"com.symeraseme.tick.plist", "com.symeraseme.poll.plist", "com.symeraseme.rescan.plist"}
	case scheduler.PlatformSystemd:
		return filepath.Join(home, ".config", "systemd", "user"), []string{"symeraseme-tick.service", "symeraseme-tick.timer", "symeraseme-poll.service", "symeraseme-poll.timer", "symeraseme-rescan.service", "symeraseme-rescan.timer"}
	default:
		return "", nil
	}
}

func isNativeSchedulerFile(name string, platform scheduler.Platform) bool {
	if platform == scheduler.PlatformLaunchd {
		return strings.HasSuffix(name, ".plist")
	}
	return strings.HasSuffix(name, ".service") || strings.HasSuffix(name, ".timer")
}

func generatedSchedulerFiles(opts Options, platform scheduler.Platform, destination string) (map[string]string, error) {
	cfg := opts.SchedulerConfig
	cfg.Platform = platform
	if cfg.OutputDir == "" {
		cfg.OutputDir = filepath.Join(destination, "schedules")
	}
	if cfg.TickHour == 0 && cfg.TickMinute == 0 {
		cfg.TickHour = 10
	}
	if len(cfg.PollHours) == 0 {
		cfg.PollHours = []int{8, 12, 16, 20}
	}
	if cfg.BinaryPath == "" {
		cfg.BinaryPath = opts.BinaryPath
	}
	if cfg.ProjectDir == "" {
		cfg.ProjectDir = opts.ProjectDir
	}
	return scheduler.Generate(cfg)
}

// Run performs detection, backup, and resumable migration. A non-nil report
// is returned with an error so callers can display the exact failed item.
func Run(ctx context.Context, opts Options) (*Report, error) {
	source, destination, err := validateRoots(opts.SourceRoot, opts.DestinationRoot)
	if err != nil {
		return nil, err
	}
	opts.SourceRoot, opts.DestinationRoot = source, destination
	configSource, configDestination, err := configRoots(opts, source, destination)
	if err != nil {
		return nil, err
	}
	if opts.Platform == "" {
		opts.Platform = scheduler.DetectPlatform()
	}
	detection, err := Detect(ctx, opts)
	if err != nil {
		return nil, err
	}
	report := &Report{Detection: detection, DryRun: opts.DryRun}
	for _, a := range detection.Artifacts {
		report.Items = append(report.Items, ItemResult{Artifact: a, Status: "planned"})
	}
	if opts.DryRun || !detection.Detected {
		report.Complete = !detection.Detected || len(detection.Artifacts) == 0
		return report, nil
	}
	if opts.CopySecrets && detection.SecretStore.Detected && !detection.SecretStore.Migratable {
		return report, errors.New("secret store was detected but no migratable SecretStore was injected")
	}

	statePath := filepath.Join(destination, ".migration-state.json")
	st, found, err := loadState(statePath, source, destination)
	if err != nil {
		return report, err
	}
	if found {
		report.Resumed = true
	}
	backupDir := st.BackupDir
	if backupDir == "" {
		backupDir = opts.BackupDir
		if backupDir == "" {
			backupDir = destination + ".migration-backup"
		}
		backupDir, err = absoluteDir(backupDir)
		if err != nil {
			return report, fmt.Errorf("resolve backup directory: %w", err)
		}
	}
	backupRoots := []string{source, destination, configSource, configDestination}
	if opts.SchedulerSource != "" {
		backupRoots = append(backupRoots, opts.SchedulerSource)
	}
	if opts.SchedulerDest != "" {
		backupRoots = append(backupRoots, opts.SchedulerDest)
	}
	if nativeRoot, _ := nativeSchedulerPaths(opts.HomeDir, opts.Platform); nativeRoot != "" {
		backupRoots = append(backupRoots, nativeRoot)
	}
	if err := validateBackupDir(backupDir, backupRoots...); err != nil {
		return report, err
	}
	if err := ensureBackup(backupDir, source, []string{configSource}, detection.Artifacts); err != nil {
		return report, err
	}
	report.BackupDir = backupDir
	if st.Version == 0 {
		st = state{Version: stateVersion, SourceRoot: source, DestinationRoot: destination, BackupDir: backupDir, Items: map[string]string{}}
	}
	if st.Items == nil {
		st.Items = map[string]string{}
	}
	if err := os.MkdirAll(destination, 0o700); err != nil {
		return report, fmt.Errorf("create destination: %w", err)
	}
	if err := writeState(statePath, st); err != nil {
		return report, err
	}

	generated, err := generatedSchedulerFiles(opts, opts.Platform, destination)
	if err != nil {
		return report, err
	}
	for i := range report.Items {
		a := report.Items[i].Artifact
		if st.Items[a.ID] == "done" {
			report.Items[i].Status = "skipped"
			continue
		}
		if opts.BeforeItem != nil {
			if err := opts.BeforeItem(a); err != nil {
				report.Items[i].Status = "failed"
				report.Items[i].Error = err.Error()
				return report, fmt.Errorf("migrate %s: %w", a.ID, err)
			}
		}
		if a.Kind == KindSecrets {
			if !opts.CopySecrets {
				report.Items[i].Status = "manual"
				report.Warnings = append(report.Warnings, "secret values were not copied; configure the Go secret store manually")
				st.Items[a.ID] = "manual"
				if err := writeState(statePath, st); err != nil {
					return report, err
				}
				continue
			}
			store := opts.SecretStore
			if store == nil {
				store = MetadataOnlySecretStore{}
			}
			if err := store.Migrate(ctx, destination); err != nil {
				report.Items[i].Status = "failed"
				report.Items[i].Error = err.Error()
				return report, fmt.Errorf("migrate secret store: %w", err)
			}
		} else {
			data, mode, err := artifactData(a, generated)
			if err != nil {
				report.Items[i].Status = "failed"
				report.Items[i].Error = err.Error()
				return report, err
			}
			if a.Kind == KindScheduler {
				wrapperDir := opts.SchedulerConfig.OutputDir
				if wrapperDir == "" {
					wrapperDir = filepath.Join(destination, "schedules")
				}
				data = []byte(strings.ReplaceAll(string(data), "__WRAPPER_DIR__", wrapperDir))
			}
			if err := writeAtomic(a.Destination, data, mode); err != nil {
				report.Items[i].Status = "failed"
				report.Items[i].Error = err.Error()
				return report, fmt.Errorf("write %s: %w", a.Destination, err)
			}
		}
		st.Items[a.ID] = "done"
		report.Items[i].Status = "done"
		if err := writeState(statePath, st); err != nil {
			return report, err
		}
	}
	report.Complete = true
	for i := range report.Items {
		if st.Items[report.Items[i].Artifact.ID] != "done" {
			report.Complete = false
		}
	}
	return report, nil
}

func artifactData(a Artifact, generated map[string]string) ([]byte, os.FileMode, error) {
	if a.Kind == KindScheduler {
		name := filepath.Base(a.Destination)
		content, ok := generated[name]
		if !ok {
			return nil, 0, fmt.Errorf("no generated scheduler content for %s", name)
		}
		mode := os.FileMode(0o644)
		if strings.HasSuffix(name, ".sh") {
			mode = 0o755
		}
		return []byte(content), mode, nil
	}
	if a.Source == "" {
		return nil, 0, fmt.Errorf("migration source missing for %s", a.ID)
	}
	info, err := os.Lstat(a.Source)
	if err != nil {
		return nil, 0, fmt.Errorf("read migration source %s: %w", a.Source, err)
	}
	if !info.Mode().IsRegular() {
		return nil, 0, fmt.Errorf("migration source is not a regular file: %s", a.Source)
	}
	data, err := os.ReadFile(a.Source)
	if err != nil {
		return nil, 0, fmt.Errorf("read migration source %s: %w", a.Source, err)
	}
	return data, info.Mode().Perm(), nil
}

func validateRoots(source, destination string) (string, string, error) {
	if source == "" || destination == "" {
		return "", "", errors.New("source and destination directories are required")
	}
	source, err := absoluteDir(source)
	if err != nil {
		return "", "", fmt.Errorf("resolve source directory: %w", err)
	}
	destination, err = absoluteDir(destination)
	if err != nil {
		return "", "", fmt.Errorf("resolve destination directory: %w", err)
	}
	if err := rejectSymlinkComponents(source); err != nil {
		return "", "", fmt.Errorf("source path is unsafe: %w", err)
	}
	if err := rejectSymlinkComponents(destination); err != nil {
		return "", "", fmt.Errorf("destination path is unsafe: %w", err)
	}
	info, err := os.Lstat(source)
	if err != nil {
		return "", "", fmt.Errorf("stat source directory: %w", err)
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return "", "", errors.New("source must be a real directory")
	}
	if destInfo, statErr := os.Lstat(destination); statErr == nil && destInfo.Mode()&os.ModeSymlink != 0 {
		return "", "", errors.New("destination must not be a symlink")
	}
	if source == destination || pathWithin(source, destination) || pathWithin(destination, source) {
		return "", "", errors.New("source and destination must be separate, non-nested directories")
	}
	return source, destination, nil
}

func absoluteDir(path string) (string, error) {
	if path == "" {
		return "", errors.New("path must not be empty")
	}
	abs, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	return filepath.Clean(abs), nil
}

func configRoots(opts Options, source, destination string) (string, string, error) {
	configSource, configDestination := source, destination
	if opts.SourceConfigRoot != "" {
		var err error
		configSource, err = absoluteDir(opts.SourceConfigRoot)
		if err != nil {
			return "", "", fmt.Errorf("resolve source config directory: %w", err)
		}
	}
	if opts.DestinationConfigRoot != "" {
		var err error
		configDestination, err = absoluteDir(opts.DestinationConfigRoot)
		if err != nil {
			return "", "", fmt.Errorf("resolve destination config directory: %w", err)
		}
	}
	if configSource == configDestination || pathWithin(configSource, configDestination) || pathWithin(configDestination, configSource) {
		return "", "", errors.New("source and destination config directories must be separate, non-nested directories")
	}
	if info, err := os.Lstat(configDestination); err == nil && info.Mode()&os.ModeSymlink != 0 {
		return "", "", errors.New("destination config directory must not be a symlink")
	}
	return configSource, configDestination, nil
}

func validateAuxiliaryPaths(opts Options) error {
	for label, value := range map[string]string{
		"home": opts.HomeDir, "scheduler source": opts.SchedulerSource, "scheduler destination": opts.SchedulerDest,
		"binary": opts.BinaryPath, "project directory": opts.ProjectDir, "scheduler output": opts.SchedulerConfig.OutputDir,
	} {
		if value != "" && !filepath.IsAbs(value) {
			return fmt.Errorf("%s must be an absolute path", label)
		}
	}
	return nil
}

func validateMigrationPaths(opts Options, source, destination, configSource, configDestination string) error {
	if err := rejectSymlinkComponents(source); err != nil {
		return fmt.Errorf("source path is unsafe: %w", err)
	}
	if err := rejectSymlinkComponents(destination); err != nil {
		return fmt.Errorf("destination path is unsafe: %w", err)
	}
	for label, path := range map[string]string{
		"source config":         configSource,
		"destination config":    configDestination,
		"scheduler source":      opts.SchedulerSource,
		"scheduler destination": opts.SchedulerDest,
		"home":                  opts.HomeDir,
	} {
		if path == "" {
			continue
		}
		if err := validateOptionalDirectoryPath(label, path); err != nil {
			return err
		}
	}
	if configSource != source && pathsOverlap(configSource, destination) {
		return errors.New("source config directory must not overlap destination")
	}
	if configDestination != destination && pathsOverlap(configDestination, source) {
		return errors.New("destination config directory must not overlap source")
	}
	return nil
}

func validateOptionalDirectoryPath(label, path string) error {
	if err := rejectSymlinkComponents(path); err != nil {
		return fmt.Errorf("%s path is unsafe: %w", label, err)
	}
	info, err := os.Lstat(path)
	if os.IsNotExist(err) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("inspect %s directory: %w", label, err)
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("%s must be a real directory", label)
	}
	return nil
}

func validateBackupDir(backup string, roots ...string) error {
	if err := rejectSymlinkComponents(backup); err != nil {
		return fmt.Errorf("backup directory is unsafe: %w", err)
	}
	for _, root := range roots {
		if root != "" && pathsOverlap(root, backup) {
			return errors.New("backup directory must be outside all migration source and destination directories")
		}
	}
	return nil
}

func pathsOverlap(left, right string) bool {
	return left == right || pathWithin(left, right) || pathWithin(right, left)
}

func rejectSymlinkComponents(path string) error {
	abs, err := filepath.Abs(path)
	if err != nil {
		return err
	}
	volume := filepath.VolumeName(abs)
	current := volume + string(filepath.Separator)
	remainder := strings.TrimPrefix(abs, current)
	if remainder == abs && volume != "" {
		current = volume
		remainder = strings.TrimPrefix(abs, volume)
		remainder = strings.TrimPrefix(remainder, string(filepath.Separator))
	}
	for index, component := range strings.Split(remainder, string(filepath.Separator)) {
		if component == "" {
			continue
		}
		current = filepath.Join(current, component)
		info, err := os.Lstat(current)
		if err != nil {
			if os.IsNotExist(err) {
				return nil
			}
			return err
		}
		if info.Mode()&os.ModeSymlink != 0 && (index > 0 || !isAllowedSystemSymlink(current)) {
			return fmt.Errorf("symlink component: %s", current)
		}
	}
	return nil
}

func isAllowedSystemSymlink(path string) bool {
	if runtime.GOOS != "darwin" {
		return false
	}
	switch filepath.Clean(path) {
	case "/etc", "/private", "/tmp", "/var":
		return true
	default:
		return false
	}
}

func pathWithin(root, candidate string) bool {
	rel, err := filepath.Rel(root, candidate)
	return err == nil && rel != ".." && !strings.HasPrefix(rel, ".."+string(filepath.Separator))
}

func pathWithinAny(candidate string, roots ...string) bool {
	for _, root := range roots {
		if root != "" && pathWithin(root, candidate) {
			return true
		}
	}
	return false
}

func exists(path string) bool {
	_, err := os.Lstat(path)
	return err == nil
}

func loadState(path, source, destination string) (state, bool, error) {
	data, err := os.ReadFile(path)
	if os.IsNotExist(err) {
		return state{}, false, nil
	}
	if err != nil {
		return state{}, false, fmt.Errorf("read migration state: %w", err)
	}
	var st state
	if err := json.Unmarshal(data, &st); err != nil {
		return state{}, false, fmt.Errorf("decode migration state: %w", err)
	}
	if st.Version != stateVersion || st.SourceRoot != source || st.DestinationRoot != destination {
		return state{}, false, errors.New("migration state does not match the requested source and destination")
	}
	if st.BackupDir == "" {
		return state{}, false, errors.New("migration state has no backup directory")
	}
	return st, true, nil
}

func ensureBackup(backup, source string, extraRoots []string, artifacts []Artifact) error {
	marker := filepath.Join(backup, ".complete.json")
	if info, err := os.Lstat(marker); err == nil {
		if !info.Mode().IsRegular() {
			return errors.New("backup completion marker is not a regular file")
		}
		data, err := os.ReadFile(marker)
		if err != nil {
			return fmt.Errorf("read backup completion marker: %w", err)
		}
		var complete struct {
			Version int    `json:"version"`
			Source  string `json:"source"`
		}
		if err := json.Unmarshal(data, &complete); err != nil || complete.Version != stateVersion || complete.Source != source {
			return errors.New("backup completion marker does not match this source")
		}
		return nil
	} else if !os.IsNotExist(err) {
		return fmt.Errorf("inspect backup completion marker: %w", err)
	}
	if exists(backup) {
		return errors.New("backup directory exists without a complete marker; refusing to overwrite it")
	}
	if err := copyTree(source, filepath.Join(backup, "source")); err != nil {
		return fmt.Errorf("backup source: %w", err)
	}
	for i, root := range extraRoots {
		if root == "" || root == source || !exists(root) {
			continue
		}
		if err := copyTree(root, filepath.Join(backup, fmt.Sprintf("config-%d", i))); err != nil {
			return fmt.Errorf("backup additional source %s: %w", root, err)
		}
	}
	externalIndex := 0
	type externalBackup struct {
		ID     string `json:"id"`
		Source string `json:"source"`
		Backup string `json:"backup"`
	}
	var external []externalBackup
	for _, a := range artifacts {
		if a.Source == "" || !exists(a.Source) || pathWithinAny(a.Source, append([]string{source}, extraRoots...)...) {
			continue
		}
		name := fmt.Sprintf("%03d-%s", externalIndex, safeName(filepath.Base(a.Source)))
		externalIndex++
		backupPath := filepath.Join("external", name)
		if err := copyOne(a.Source, filepath.Join(backup, backupPath)); err != nil {
			return fmt.Errorf("backup %s: %w", a.ID, err)
		}
		external = append(external, externalBackup{ID: a.ID, Source: a.Source, Backup: backupPath})
	}
	manifest := struct {
		Version   int              `json:"version"`
		Source    string           `json:"source"`
		Artifacts []string         `json:"artifacts"`
		External  []externalBackup `json:"external"`
	}{Version: stateVersion, Source: source, External: external}
	for _, a := range artifacts {
		manifest.Artifacts = append(manifest.Artifacts, a.ID)
	}
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	if err := writeAtomic(marker, data, 0o600); err != nil {
		return fmt.Errorf("finalize backup: %w", err)
	}
	return nil
}

func copyTree(source, destination string) error {
	if err := os.MkdirAll(destination, 0o700); err != nil {
		return err
	}
	return filepath.WalkDir(source, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(source, path)
		if err != nil {
			return err
		}
		target := destination
		if rel != "." {
			target = filepath.Join(destination, rel)
		}
		if entry.IsDir() {
			return os.MkdirAll(target, 0o700)
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("refusing symlink in source: %s", path)
		}
		return copyOne(path, target)
	})
}

func copyOne(source, destination string) error {
	if err := rejectSymlinkComponents(source); err != nil {
		return fmt.Errorf("source path is unsafe: %w", err)
	}
	info, err := os.Lstat(source)
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() {
		return fmt.Errorf("source is not a regular file: %s", source)
	}
	data, err := os.ReadFile(source)
	if err != nil {
		return err
	}
	return writeAtomic(destination, data, info.Mode().Perm())
}

func writeState(path string, st state) error {
	data, err := json.MarshalIndent(st, "", "  ")
	if err != nil {
		return fmt.Errorf("encode migration state: %w", err)
	}
	return writeAtomic(path, data, 0o600)
}

func writeAtomic(path string, data []byte, mode os.FileMode) error {
	if err := rejectSymlinkComponents(path); err != nil {
		return fmt.Errorf("destination path is unsafe: %w", err)
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}
	tmp, err := os.CreateTemp(filepath.Dir(path), ".migration-write-*")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	defer os.Remove(tmpPath)
	if err := tmp.Chmod(mode); err != nil {
		tmp.Close()
		return err
	}
	if _, err := tmp.Write(data); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmpPath, path)
}

func safeName(value string) string {
	value = strings.ReplaceAll(value, string(filepath.Separator), "_")
	value = strings.ReplaceAll(value, "..", "_")
	if value == "" || value == "." {
		return "artifact"
	}
	return value
}
