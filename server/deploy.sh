#!/usr/bin/env bash
# ═══════════════════════════════════════════════════════════════
# NEXUS Sidecar — One-command deployment script
# ═══════════════════════════════════════════════════════════════
#
# Usage:
#   ./deploy.sh              # Deploy with Docker (recommended)
#   ./deploy.sh --bare       # Deploy without Docker (bare metal)
#
# Prerequisites:
#   - Docker + Docker Compose (for Docker mode)
#   - Python 3.12+ (for bare metal mode)
#   - The .env file at server/sidecar/.env with OAuth credentials
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

MODE="${1:-docker}"

echo "╔═══════════════════════════════════════════════════════════════╗"
echo "║  NEXUS Sidecar Deployment                                     ║"
echo "╚═══════════════════════════════════════════════════════════════╝"
echo ""
echo "Mode: $MODE"
echo "Directory: $SCRIPT_DIR"
echo ""

# Check for .env file
if [ ! -f sidecar/.env ]; then
    echo "⚠️  No .env file found at sidecar/.env"
    echo "   Copy env.example to .env and fill in your OAuth credentials:"
    echo "   cp sidecar/env.example sidecar/.env"
    echo "   nano sidecar/.env"
    echo ""
    echo "   Required variables:"
    echo "   - GOOGLE_CLIENT_ID / GOOGLE_CLIENT_SECRET"
    echo "   - GITHUB_CLIENT_ID / GITHUB_CLIENT_SECRET"
    echo "   - NEXUS_ENCRYPTION_KEY (generate: python -c \"from cryptography.fernet import Fernet; print(Fernet.generate_key().decode())\")"
    echo ""
    exit 1
fi

# Generate encryption key if not set
if ! grep -q "NEXUS_ENCRYPTION_KEY=." sidecar/.env; then
    echo "Generating NEXUS_ENCRYPTION_KEY..."
    KEY=$(python3 -c "from cryptography.fernet import Fernet; print(Fernet.generate_key().decode())" 2>/dev/null || echo "")
    if [ -n "$KEY" ]; then
        echo "NEXUS_ENCRYPTION_KEY=$KEY" >> sidecar/.env
        echo "✅ Encryption key added to .env"
    else
        echo "⚠️  Could not generate encryption key. Set NEXUS_ENCRYPTION_KEY manually."
    fi
fi

if [ "$MODE" = "--bare" ]; then
    # ─── Bare metal deployment ───────────────────────────────
    echo "📦 Bare metal deployment (no Docker)"
    echo ""

    if [ ! -d ".venv" ]; then
        echo "Creating virtual environment..."
        python3 -m venv .venv
    fi

    echo "Installing dependencies..."
    .venv/bin/pip install -q -r sidecar/requirements.txt

    echo ""
    echo "Starting sidecar..."
    echo "  cd sidecar && ../.venv/bin/uvicorn sidecar:app --host 0.0.0.0 --port 8443"
    echo ""

    cd sidecar
    exec ../.venv/bin/uvicorn sidecar:app --host 0.0.0.0 --port 8443

else
    # ─── Docker deployment ───────────────────────────────────
    echo "📦 Docker deployment"
    echo ""

    if ! command -v docker &> /dev/null; then
        echo "❌ Docker is not installed. Install it first:"
        echo "   curl -fsSL https://get.docker.com | sh"
        exit 1
    fi

    if ! docker compose version &> /dev/null; then
        echo "❌ Docker Compose v2 is not installed."
        exit 1
    fi

    echo "Building and starting containers..."
    docker compose up -d --build

    echo ""
    echo "Waiting for sidecar to start..."
    sleep 3

    # Health check
    if curl -s http://localhost:8443/health | grep -q '"ok": true'; then
        echo "✅ Sidecar is healthy!"
        echo ""
        echo "Endpoints:"
        echo "  Health:     http://localhost:8443/health"
        echo "  Register:   POST http://localhost:8443/api/register"
        echo "  GitHub auth: POST http://localhost:8443/auth/github"
        echo "  Google auth: POST http://localhost:8443/auth/google"
        echo "  WebSocket:  ws://$(hostname -I | awk '{print $1}'):8443/ws"
        echo ""
        echo "n8n should call:"
        echo "  http://sidecar:8443/auth/github  (Docker internal DNS)"
        echo "  or"
        echo "  http://localhost:8443/auth/github  (if n8n is on bare metal)"
    else
        echo "❌ Sidecar health check failed. Check logs:"
        echo "   docker compose logs sidecar"
        exit 1
    fi

    echo ""
    echo "To view logs:   docker compose logs -f sidecar"
    echo "To stop:         docker compose down"
fi
