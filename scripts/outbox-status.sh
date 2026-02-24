#!/bin/bash
# Print outbox status from SQLite directly.
# Usage: bash scripts/outbox-status.sh

DB="$HOME/Library/Application Support/com.taisun.electron/events.db"

if [ ! -f "$DB" ]; then
  echo "ERROR: events.db not found at: $DB"
  exit 1
fi

echo "=== OUTBOX STATUS ==="
echo ""

echo "-- Counts --"
sqlite3 "$DB" "
  SELECT
    SUM(CASE WHEN status='pending' THEN 1 ELSE 0 END) AS pending,
    SUM(CASE WHEN status='delivered' THEN 1 ELSE 0 END) AS delivered,
    COUNT(*) AS total
  FROM outbox;
"

echo ""
echo "-- Pending rows (up to 10) --"
sqlite3 -header -column "$DB" "
  SELECT outbox_id, status, attempts, last_error, next_retry_at, created_at
  FROM outbox
  WHERE status = 'pending'
  ORDER BY outbox_id ASC
  LIMIT 10;
"

echo ""
echo "-- Recent delivered (up to 5) --"
sqlite3 -header -column "$DB" "
  SELECT outbox_id, status, attempts, created_at
  FROM outbox
  WHERE status = 'delivered'
  ORDER BY outbox_id DESC
  LIMIT 5;
"

echo ""
echo "-- Event count --"
sqlite3 "$DB" "SELECT COUNT(*) || ' total events' FROM events;"
