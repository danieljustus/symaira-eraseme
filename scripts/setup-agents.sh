#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
SKILLS_DIR="${PROJECT_ROOT}/skills"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m'

info() { echo -e "${BLUE}ℹ${NC} $1"; }
success() { echo -e "${GREEN}✓${NC} $1"; }
warn() { echo -e "${YELLOW}⚠${NC} $1"; }
error() { echo -e "${RED}✗${NC} $1"; }

usage() {
    cat <<EOF
Usage: $(basename "$0") [OPTIONS]

Setup AI agent integrations for Symaira EraseMe.

OPTIONS:
    --agent <name>    Setup specific agent (claude, cursor, windsurf, continue, cline, aider, codex, copilot, hermes, all)
    --list            List available agents
    --help            Show this help message

EXAMPLES:
    $(basename "$0") --agent all          # Setup all agents
    $(basename "$0") --agent cursor       # Setup only Cursor
    $(basename "$0") --agent claude       # Setup only Claude Code
EOF
}

list_agents() {
    echo "Available agents:"
    echo "  - claude     (Claude Code)"
    echo "  - cursor     (Cursor IDE)"
    echo "  - windsurf   (Windsurf/Codeium)"
    echo "  - continue   (Continue.dev)"
    echo "  - cline      (Cline)"
    echo "  - aider      (Aider)"
    echo "  - codex      (OpenAI Codex CLI)"
    echo "  - copilot    (GitHub Copilot CLI)"
    echo "  - hermes     (Hermes)"
    echo "  - openclaw   (OpenClaw)"
    echo "  - all        (All of the above)"
}

setup_claude() {
    info "Setting up Claude Code..."
    
    if [ -d "${PROJECT_ROOT}/.claude/skills/symaira-eraseme" ]; then
        success "Claude Code already configured"
        return 0
    fi
    
    mkdir -p "${PROJECT_ROOT}/.claude/skills"
    ln -sf "../../skills" "${PROJECT_ROOT}/.claude/skills/symaira-eraseme"
    success "Claude Code configured at .claude/skills/symaira-eraseme"
    info "Run 'claude' in project directory to use"
}

setup_cursor() {
    info "Setting up Cursor..."
    
    mkdir -p "${PROJECT_ROOT}/.cursor/skills"
    
    if [ ! -L "${PROJECT_ROOT}/.cursor/skills/symaira-eraseme" ]; then
        ln -sf "../../skills" "${PROJECT_ROOT}/.cursor/skills/symaira-eraseme"
        success "Cursor skills configured"
    fi
    
    if [ ! -f "${PROJECT_ROOT}/.cursor/rules/symaira-eraseme.mdc" ]; then
        mkdir -p "${PROJECT_ROOT}/.cursor/rules"
        cp "${PROJECT_ROOT}/examples/cursor/symaira-eraseme.mdc" "${PROJECT_ROOT}/.cursor/rules/" 2>/dev/null || \
            warn "No .mdc template found, skipping rules setup"
    fi
    
    success "Cursor configured"
}

setup_windsurf() {
    info "Setting up Windsurf..."
    
    mkdir -p "${PROJECT_ROOT}/.windsurf/skills"
    
    if [ ! -L "${PROJECT_ROOT}/.windsurf/skills/symaira-eraseme" ]; then
        ln -sf "../../skills" "${PROJECT_ROOT}/.windsurf/skills/symaira-eraseme"
        success "Windsurf skills configured"
    fi
    
    if [ ! -d "${PROJECT_ROOT}/.windsurf/rules" ]; then
        mkdir -p "${PROJECT_ROOT}/.windsurf/rules"
        cp "${PROJECT_ROOT}/examples/windsurf/symaira-eraseme.md" "${PROJECT_ROOT}/.windsurf/rules/" 2>/dev/null || \
            warn "No rules template found"
    fi
    
    success "Windsurf configured"
}

setup_continue() {
    info "Setting up Continue..."
    
    mkdir -p "${PROJECT_ROOT}/.continue/rules"
    
    if [ ! -f "${PROJECT_ROOT}/.continue/rules/symaira-eraseme.md" ]; then
        cp "${PROJECT_ROOT}/examples/continue/.continue/rules/symaira-eraseme.md" \
           "${PROJECT_ROOT}/.continue/rules/symaira-eraseme.md"
        success "Continue rules configured"
    fi
    
    if [ ! -f "${PROJECT_ROOT}/.continuerc.json" ]; then
        cat > "${PROJECT_ROOT}/.continuerc.json" <<'EOF'
{
  "rules": [
    ".continue/rules/symaira-eraseme.md"
  ]
}
EOF
        success "Continue project config created (.continuerc.json)"
    fi
}

setup_cline() {
    info "Setting up Cline..."
    
    mkdir -p "${PROJECT_ROOT}/.clinerules"
    
    if [ ! -f "${PROJECT_ROOT}/.clinerules/00-symaira-eraseme.md" ]; then
        cp "${PROJECT_ROOT}/examples/cline/.clinerules/00-symaira-eraseme.md" \
           "${PROJECT_ROOT}/.clinerules/00-symaira-eraseme.md"
        success "Cline rules configured"
    fi
}

setup_aider() {
    info "Setting up Aider..."
    
    if [ ! -f "${PROJECT_ROOT}/CONVENTIONS.md" ]; then
        cp "${PROJECT_ROOT}/examples/aider/CONVENTIONS.md" \
           "${PROJECT_ROOT}/CONVENTIONS.md"
        success "Aider conventions created (CONVENTIONS.md)"
    fi
    
    if [ ! -f "${PROJECT_ROOT}/.aider.conf.yml" ]; then
        cat > "${PROJECT_ROOT}/.aider.conf.yml" <<'EOF'
read:
  - CONVENTIONS.md
EOF
        success "Aider config created (.aider.conf.yml)"
    fi
}

setup_codex() {
    info "Setting up Codex CLI..."
    
    if [ -d "${PROJECT_ROOT}/.agents/skills/symaira-eraseme" ]; then
        success "Codex CLI already configured (.agents/skills/)"
    else
        mkdir -p "${PROJECT_ROOT}/.agents/skills"
        ln -sf "../../skills" "${PROJECT_ROOT}/.agents/skills/symaira-eraseme"
        success "Codex CLI configured"
    fi
    
    if [ ! -d "${PROJECT_ROOT}/agents" ]; then
        mkdir -p "${PROJECT_ROOT}/agents"
        cat > "${PROJECT_ROOT}/agents/openai.yaml" <<'EOF'
interface:
  display_name: "EraseMe"
  short_description: "GDPR/CCPA data broker removal tool"
  brand_color: "#3B82F6"
policy:
  allow_implicit_invocation: true
EOF
        success "Codex metadata created (agents/openai.yaml)"
    fi
}

setup_copilot() {
    info "Setting up GitHub Copilot CLI..."
    
    if [ -d "${PROJECT_ROOT}/.agents/skills/symaira-eraseme" ]; then
        success "GitHub Copilot CLI already configured (.agents/skills/)"
    else
        mkdir -p "${PROJECT_ROOT}/.agents/skills"
        ln -sf "../../skills" "${PROJECT_ROOT}/.agents/skills/symaira-eraseme"
        success "GitHub Copilot CLI configured"
    fi
}

setup_hermes() {
    info "Setting up Hermes..."
    info "Hermes requires manual installation to ~/.hermes/skills/"
    info "Run: hermes skills install https://raw.githubusercontent.com/danieljustus/Symaira-EraseMe/main/skills/SKILL.md"
    info "Or copy manually:"
    info "  mkdir -p ~/.hermes/skills/privacy-tools/symaira-eraseme"
    info "  cp skills/SKILL.md ~/.hermes/skills/privacy-tools/symaira-eraseme/"
}

setup_openclaw() {
    info "Setting up OpenClaw..."
    info "OpenClaw requires manual YAML installation"
    info "Copy examples/openclaw/symeraseme.yaml to ~/.config/openclaw/skills/"
    info "Then run: openclaw skill load symeraseme"
}

AGENT=""

while [[ $# -gt 0 ]]; do
    case $1 in
        --agent)
            AGENT="$2"
            shift 2
            ;;
        --list)
            list_agents
            exit 0
            ;;
        --help)
            usage
            exit 0
            ;;
        *)
            error "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

if [ -z "$AGENT" ]; then
    error "No agent specified"
    usage
    exit 1
fi

case "$AGENT" in
    claude)
        setup_claude
        ;;
    cursor)
        setup_cursor
        ;;
    windsurf)
        setup_windsurf
        ;;
    continue)
        setup_continue
        ;;
    cline)
        setup_cline
        ;;
    aider)
        setup_aider
        ;;
    codex)
        setup_codex
        ;;
    copilot)
        setup_copilot
        ;;
    hermes)
        setup_hermes
        ;;
    openclaw)
        setup_openclaw
        ;;
    all)
        setup_claude
        setup_cursor
        setup_windsurf
        setup_continue
        setup_cline
        setup_aider
        setup_codex
        setup_copilot
        setup_hermes
        setup_openclaw
        ;;
    *)
        error "Unknown agent: $AGENT"
        list_agents
        exit 1
        ;;
esac

success "Setup complete!"
info "See AGENTS.md for usage instructions."
