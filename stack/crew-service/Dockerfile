FROM python:3.12-slim

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    curl \
    git \
    && rm -rf /var/lib/apt/lists/*

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY main.py crew_safety.py crew_tasks.py crew_memory.py crew_grants.py ./

RUN useradd --create-home --uid 10001 crew
USER crew

EXPOSE 8400

ENV PYTHONUNBUFFERED=1 \
    CREWAI_DISABLE_TELEMETRY=true \
    OTEL_SDK_DISABLED=true \
    CREW_CONTAINER=1 \
    CREW_SERVICE_HOST=127.0.0.1 \
    CREW_SERVICE_PORT=8400

HEALTHCHECK --interval=15s --timeout=5s --retries=5 \
  CMD curl -fsS "http://127.0.0.1:${CREW_SERVICE_PORT}/health" || exit 1

# Host defaults to 127.0.0.1; compose overrides to 0.0.0.0 *inside* the
# container so the 127.0.0.1-bound host port mapping can reach it.
CMD ["sh", "-c", "exec uvicorn main:app --host \"$CREW_SERVICE_HOST\" --port \"$CREW_SERVICE_PORT\""]
