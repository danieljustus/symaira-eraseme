// Command symeraseme is the Go-port entrypoint of Symaira EraseMe.
//
// This is the empty-but-wired skeleton (milestone v1.0.0): the CLI shell,
// version handshake, config and logging baselines exist; product commands
// land package by package. The Python tree stays the production
// implementation until the cutover.
package main

import (
	"context"
	"os"

	"github.com/spf13/cobra"

	"github.com/danieljustus/symaira-corekit/logkit"

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
	return &cobra.Command{
		Use:   "mcp",
		Short: "Run the MCP JSON-RPC interface",
		Args:  cobra.NoArgs,
		RunE: func(cmd *cobra.Command, _ []string) error {
			return mcp.NewServer(mcp.ContractHandler()).ServeStdio(context.Background(), cmd.InOrStdin(), cmd.OutOrStdout())
		},
	}
}

func newServeAlias() *cobra.Command {
	return &cobra.Command{
		Use:    "serve",
		Hidden: true,
		Short:  "Deprecated: use mcp instead",
		Args:   cobra.NoArgs,
		RunE: func(cmd *cobra.Command, args []string) error {
			cmd.PrintErrln("symeraseme serve is deprecated and will be removed. Please use symeraseme mcp instead.")
			return mcp.NewServer(mcp.ContractHandler()).ServeStdio(context.Background(), cmd.InOrStdin(), cmd.OutOrStdout())
		},
	}
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
		os.Exit(1)
	}
}
