# NEXUS Sidecar — lightweight FastAPI bridge
# ~35 MB RAM, text-only, no ML models
# Runs alongside n8n on the same server

FROM python:3.12-slim

WORKDIR /app

# Install dependencies first (better layer caching)
COPY sidecar/requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy the sidecar code
COPY sidecar/ .

# Create data directory for SQLite DB
RUN mkdir -p /data
ENV NEXUS_DB_PATH=/data/nexus_credentials.db

# Expose the sidecar port
EXPOSE 8443

# Health check
HEALTHCHECK --interval=30s --timeout=5s --retries=3 \
    CMD python -c "import urllib.request; urllib.request.urlopen('http://localhost:8443/health')" || exit 1

# Run the sidecar
CMD ["uvicorn", "sidecar:app", "--host", "0.0.0.0", "--port", "8443"]
