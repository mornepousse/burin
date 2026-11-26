#!/usr/bin/env bash
# Puts the report relay (relay/worker.js) on Cloudflare: the Worker, its mail binding, the author's
# address as a secret, and the route https://$ZONE/api/report. Run from the app folder:
#   CLOUDFLARE_API_TOKEN=... CLOUDFLARE_ACCOUNT_ID=... relay/deploy.sh
# The address comes from a file kept off the repository (.backup/report-email); it must be a
# verified destination of the zone's Email Routing, and Email Routing must be on.
set -euo pipefail
cd "$(dirname "$0")/.."
: "${CLOUDFLARE_API_TOKEN:?set CLOUDFLARE_API_TOKEN}"
: "${CLOUDFLARE_ACCOUNT_ID:?set CLOUDFLARE_ACCOUNT_ID}"
ZONE=${RELAY_ZONE:-burin.cc}
NAME=${RELAY_NAME:-burin-relay}
FROM=${RELAY_FROM:-reports@$ZONE}
REPORT_FILE=${BURIN_REPORT_EMAIL_FILE:-.backup/report-email}
TO=$(tr -d ' \n' < "$REPORT_FILE")
API=https://api.cloudflare.com/client/v4
AUTH=(-H "Authorization: Bearer $CLOUDFLARE_API_TOKEN")

meta=$(mktemp)
chmod 600 "$meta"
trap 'rm -f "$meta"' EXIT
cat > "$meta" <<EOF
{"main_module":"worker.js","compatibility_date":"2026-11-01",
 "bindings":[{"type":"send_email","name":"MAIL"},
             {"type":"plain_text","name":"REPORT_FROM","text":"$FROM"},
             {"type":"secret_text","name":"REPORT_TO","text":"$TO"}]}
EOF
echo "Uploading the Worker $NAME..."
curl -sS -X PUT "$API/accounts/$CLOUDFLARE_ACCOUNT_ID/workers/scripts/$NAME" "${AUTH[@]}" \
    -F "metadata=@$meta;type=application/json" \
    -F "worker.js=@relay/worker.js;type=application/javascript+module" \
    | grep -o '"success": *[a-z]*\|"message": *"[^"]*"'

zone_id=$(curl -sS "$API/zones?name=$ZONE" "${AUTH[@]}" | grep -o '"id": *"[0-9a-f]\{32\}"' | head -1 | grep -o '[0-9a-f]\{32\}')
pattern="$ZONE/api/report"
if curl -sS "$API/zones/$zone_id/workers/routes" "${AUTH[@]}" | grep -q "\"pattern\": *\"$pattern\""; then
    echo "The route $pattern is already there."
else
    echo "Adding the route $pattern..."
    curl -sS -X POST "$API/zones/$zone_id/workers/routes" "${AUTH[@]}" -H "Content-Type: application/json" \
        --data "{\"pattern\":\"$pattern\",\"script\":\"$NAME\"}" | grep -o '"success": *[a-z]*\|"message": *"[^"]*"'
fi
