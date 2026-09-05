// Command symeraseme is the Go-port entrypoint of Symaira EraseMe.
//
// The CLI and MCP contract are implemented in Go. The Python tree remains
// available during the bounded cutover milestone until issue #731 removes it.
package main

import (
	"context"
	"errors"
	"fmt"
	"net"
	"net/http"
	"os"
	"os/signal"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"time"

	"github.com/spf13/cobra"

	"github.com/danieljustus/symaira-corekit/logkit"

	"github.com/danieljustus/symaira-eraseme/internal/config"
	"github.com/danieljustus/symaira-eraseme/internal/identity"
	"github.com/danieljustus/symaira-eraseme/internal/mcp"
	"github.com/danieljustus/symaira-eraseme/internal/migration"
	"github.com/danieljustus/symaira-eraseme/internal/version"
)

// version is replaced at build time by the Makefile's VERSION value.
// Default is "dev" until a Go-port tag exists (exact-match git describe
// falls back to dev on the untagged skeleton).
var versionValue = "dev"

func newRootCommand() *cobra.Command {
	root := &cobra.Command{
		Use:           "symeraseme",
		Short:         "Automated data broker removal tool",
		Long:          "symeraseme is the Go-port command-line entrypoint of Symaira EraseMe. The CLI vocabulary is settled in docs/mcp-contract.md §5; commands land as their port issues are implemented.",
		Version:       versionValue,
		SilenceUsage:  true,
		SilenceErrors: true,
	}
	root.PersistentFlags().String("output", "text", "output format: text or json")
	root.AddCommand(newVersionCommand())
	addCommandSurface(root)
	root.AddCommand(migration.NewCommand())
	root.AddCommand(newMCPCommand())
	root.AddCommand(newServeAlias())
	root.AddCommand(newCompletionCommand())
	return root
}

func newMCPCommand() *cobra.Command {
	return newMCPServerCommand("mcp", false)
}

func newServeAlias() *cobra.Command {
	return newMCPServerCommand("serve", true)
}

func newMCPServerCommand(use string, deprecated bool) *cobra.Command {
	var host string
	var port int
	var allowRemote bool
	var stdio bool

	cmd := &cobra.Command{
		Use:    use,
		Hidden: deprecated,
		Short:  "Run the MCP JSON-RPC interface",
		Args:   cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			if !cmd.Flags().Changed("port") || !cmd.Flags().Changed("allow-remote") {
				cfg, err := config.Load().Load()
				if err != nil {
					return err
				}
				if !cmd.Flags().Changed("port") {
					port = cfg.Port
				}
				if !cmd.Flags().Changed("allow-remote") {
					allowRemote = cfg.AllowRemote
				}
			}
			if deprecated {
				cmd.PrintErrln("symeraseme serve is deprecated and will be removed. Please use symeraseme mcp instead.")
			}
			if stdio {
				return mcp.NewServer(mcp.ContractHandler()).ServeStdio(cmd.Context(), cmd.InOrStdin(), cmd.OutOrStdout())
			}
			return runMCPHTTP(cmd.Context(), host, port, allowRemote)
		},
	}
	cmd.Flags().StringVar(&host, "host", "127.0.0.1", "MCP HTTP bind host")
	cmd.Flags().IntVar(&port, "port", 8000, "MCP HTTP bind port")
	cmd.Flags().BoolVar(&allowRemote, "allow-remote", false, "allow non-loopback HTTP binds")
	cmd.Flags().BoolVar(&stdio, "stdio", false, "use newline-delimited JSON-RPC on stdin/stdout")
	return cmd
}

func runMCPHTTP(ctx context.Context, host string, port int, allowRemote bool) error {
	host = strings.Trim(host, "[]")
	if host == "" {
		host = "127.0.0.1"
	}
	if port < 1 || port > 65535 {
		return fmt.Errorf("invalid MCP port %d: must be between 1 and 65535", port)
	}
	if !allowRemote && !isLoopbackHost(host) {
		return fmt.Errorf("refusing non-loopback MCP bind %q without --allow-remote", host)
	}

	token, _, err := writeMCPAuthToken()
	if err != nil {
		return fmt.Errorf("write MCP auth token: %w", err)
	}

	rpc := mcp.NewServerWithOptions(mcp.ContractHandler(), mcp.ServerOptions{
		AuthToken:   token,
		AllowRemote: allowRemote,
	})
	server := &http.Server{
		Addr:              net.JoinHostPort(host, strconv.Itoa(port)),
		Handler:           rpc,
		ReadHeaderTimeout: 5 * time.Second,
	}

	signalCtx, stop := signal.NotifyContext(ctx, os.Interrupt, syscall.SIGTERM)
	defer stop()
	serveErr := make(chan error, 1)
	go func() { serveErr <- server.ListenAndServe() }()

	select {
	case err := <-serveErr:
		if errors.Is(err, http.ErrServerClosed) {
			return nil
		}
		return err
	case <-signalCtx.Done():
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		if err := server.Shutdown(shutdownCtx); err != nil {
			return err
		}
		return nil
	}
}

func isLoopbackHost(host string) bool {
	host = strings.Trim(host, "[]")
	if strings.EqualFold(host, "localhost") {
		return true
	}
	ip := net.ParseIP(host)
	return ip != nil && ip.IsLoopback()
}

func writeMCPAuthToken() (string, string, error) {
	dir, err := identity.DefaultConsentDir()
	if err != nil {
		return "", "", err
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return "", "", err
	}
	token := identity.RandomURLSafe(32)
	path := filepath.Join(dir, "mcp_token")
	if err := os.WriteFile(path, []byte(token), 0o600); err != nil {
		return "", "", err
	}
	if err := os.Chmod(path, 0o600); err != nil {
		return "", "", err
	}
	return token, path, nil
}

func newVersionCommand() *cobra.Command {
	var jsonOutput bool
	cmd := &cobra.Command{
		Use:   "version",
		Short: "Show the installed Symaira EraseMe version",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			info := version.Info(versionValue)
			if jsonOutput {
				return info.Write(cmd.OutOrStdout())
			}
			_, err := cmd.OutOrStdout().Write([]byte(info.String() + "\n"))
			return err
		},
	}
	cmd.Flags().BoolVar(&jsonOutput, "json", false, "print the versionkit handshake payload (JSON)")
	return cmd
}

func main() {
	logkit.InitDefault("symeraseme")
	if err := newRootCommand().Execute(); err != nil {
		_, _ = fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
