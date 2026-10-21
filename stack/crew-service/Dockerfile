FROM python:3.12-slim

WORKDIR /app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    curl \
    && rm -rf /var/lib/apt/lists/*

# Copy requirements and install
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Copy application code
COPY . .

# Expose port 8400
EXPOSE 8400

# Set environment variables
ENV PYTHONUNBUFFERED=1
ENV PORT=8400

# Run the application
CMD ["uvicorn", "main:app", "--host", "0.0.0.0", "--port", "8400"]
